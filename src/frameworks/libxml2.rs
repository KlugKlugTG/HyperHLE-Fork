//! Host-side libxml2 implementation for HyperHLE.
//!
//! Uses a built-in minimal XML parser; all document/reader
//! state lives in a global host-side table keyed by opaque
//! handles.  Only returned xmlChar* strings are allocated in
//! guest memory.

use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::fs::GuestPath;
use crate::mem::{GuestUSize, MutPtr, Ptr};
use crate::Environment;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/usr/lib/libxml2.2.dylib",
    aliases: &["/usr/lib/libxml2.dylib"],
    class_exports: &[],
    constant_exports: &[CONSTANTS],
    function_exports: &[FUNCTIONS],
};

const FUNCTIONS: FunctionExports = &[
    export_c_func!(xmlNewParserCtxt()),
    export_c_func!(xmlClearParserCtxt()),
    export_c_func!(xmlNewDoc(_)),
    export_c_func!(xmlNewNode(_, _)),
    export_c_func!(xmlDocSetRootElement(_, _)),
    export_c_func!(xmlSaveFile(_, _)),
    export_c_func!(xmlAddChild(_, _)),
    export_c_func!(xmlNewProp(_, _, _)),
    export_c_func!(xmlSetProp(_, _, _)),
    export_c_func!(xmlNewText(_)),
    export_c_func!(xmlNewChild(_, _, _, _)),
    export_c_func!(xmlNodeSetContent(_, _)),
    export_c_func!(xmlCtxtReadMemory(_, _, _, _, _, _)),
    export_c_func!(xmlReadFile(_, _, _)),
    export_c_func!(xmlReadMemory(_, _, _, _, _)),
    export_c_func!(xmlParseMemory(_, _)),
    export_c_func!(xmlDocGetRootElement(_)),
    export_c_func!(xmlFree(_)),
    export_c_func!(xmlCtxtGetLastError(_)),
    export_c_func!(xmlFreeDoc(_)),
    export_c_func!(xmlCleanupParser()),
    export_c_func!(xmlFreeParserCtxt(_)),
    export_c_func!(xmlGetProp(_, _)),
    export_c_func!(xmlHasProp(_, _)),
    export_c_func!(xmlStrcmp(_, _)),
    export_c_func!(xmlNodeGetContent(_)),
    // TextReader API
    export_c_func!(xmlReaderForFile(_, _, _)),
    export_c_func!(xmlFreeTextReader(_)),
    export_c_func!(xmlTextReaderRead(_)),
    export_c_func!(xmlTextReaderNodeType(_)),
    export_c_func!(xmlTextReaderConstName(_)),
    export_c_func!(xmlTextReaderConstValue(_)),
    export_c_func!(xmlTextReaderName(_)),
    export_c_func!(xmlTextReaderValue(_)),
    export_c_func!(xmlTextReaderReadInnerXml(_)),
    export_c_func!(xmlTextReaderReadOuterXml(_)),
    export_c_func!(xmlTextReaderGetAttribute(_, _)),
    export_c_func!(xmlTextReaderAttributeCount(_)),
    export_c_func!(xmlTextReaderDepth(_)),
    export_c_func!(xmlTextReaderHasAttributes(_)),
    export_c_func!(xmlTextReaderIsEmptyElement(_)),
    export_c_func!(xmlTextReaderNext(_)),
    export_c_func!(xmlTextReaderNextSibling(_)),
    export_c_func!(xmlTextReaderLocalName(_)),
    export_c_func!(xmlTextReaderPrefix(_)),
    export_c_func!(xmlTextReaderNamespaceUri(_)),
    // DOM tree
    export_c_func!(xmlChildrenNode(_)),
    export_c_func!(xmlGetLastChild(_)),
    export_c_func!(xmlNextElementSibling(_)),
    export_c_func!(xmlFirstElementChild(_)),
    export_c_func!(xmlNodeListGetString(_, _, _)),
    // String utilities
    export_c_func!(xmlStrdup(_)),
    export_c_func!(xmlStrlen(_)),
    export_c_func!(xmlStrsub(_, _, _)),
];

pub const CONSTANTS: ConstantExports = &[
    ("_xmlIndentTreeOutput", HostConstant::NullPtr),
    ("_xmlSaveNoEmptyTags", HostConstant::NullPtr),
];

// ============================================================
// Constants
// ============================================================

/// All host-side handles live above this base so they never
/// collide with real guest memory pointers.
const HANDLE_BASE: u32 = 0x1000_0000;

const XML_READER_TYPE_ELEMENT: i32 = 1;
const XML_READER_TYPE_TEXT: i32 = 3;
const XML_READER_TYPE_END_ELEMENT: i32 = 15;

// ============================================================
// Host-side data types
// ============================================================

#[derive(Debug, Clone)]
struct XmlAttr {
    name: Vec<u8>,
    value: Vec<u8>,
}

/// One element node in the DOM pool.
#[derive(Debug, Clone)]
struct XmlNodeData {
    name: Vec<u8>,
    content: Vec<u8>,
    attrs: Vec<XmlAttr>,
    parent: u32,      // 0 = none
    first_child: u32, // 0 = none
    last_child: u32,  // 0 = none
    next_sib: u32,    // 0 = none
    prev_sib: u32,    // 0 = none
    doc_handle: u32,
}

#[derive(Debug)]
struct XmlDocData {
    root: u32,
}

/// One step produced by the pull-parser.
#[derive(Debug, Clone)]
enum ReaderEvent {
    Start {
        name: Vec<u8>,
        attrs: Vec<XmlAttr>,
        depth: i32,
        is_empty: bool,
    },
    End {
        name: Vec<u8>,
        depth: i32,
    },
    Text {
        value: Vec<u8>,
        depth: i32,
    },
}

#[derive(Debug)]
struct XmlReaderData {
    events: Vec<ReaderEvent>,
    /// Current position; -1 = before first event.
    pos: i64,
}

// ============================================================
// Global state
// ============================================================

struct XmlState {
    next_id: u32,
    handles: HashSet<u32>,
    nodes: HashMap<u32, XmlNodeData>,
    docs: HashMap<u32, XmlDocData>,
    readers: HashMap<u32, XmlReaderData>,
}

impl XmlState {
    fn new() -> Self {
        XmlState {
            next_id: HANDLE_BASE,
            handles: HashSet::new(),
            nodes: HashMap::new(),
            docs: HashMap::new(),
            readers: HashMap::new(),
        }
    }

    fn alloc(&mut self) -> u32 {
        let h = self.next_id;
        self.next_id += 1;
        self.handles.insert(h);
        h
    }

    fn is_handle(&self, h: u32) -> bool {
        self.handles.contains(&h)
    }

    fn release(&mut self, h: u32) {
        self.nodes.remove(&h);
        self.docs.remove(&h);
        self.readers.remove(&h);
        self.handles.remove(&h);
    }
}

static XML: Mutex<Option<XmlState>> = Mutex::new(None);

fn with_xml<F, R>(f: F) -> R
where
    F: FnOnce(&mut XmlState) -> R,
{
    let mut g = XML.lock().unwrap();
    if g.is_none() {
        *g = Some(XmlState::new());
    }
    f(g.as_mut().unwrap())
}

// ============================================================
// Guest memory helpers
// ============================================================

fn read_guest_bytes(env: &Environment, ptr: u32, len: u32) -> Vec<u8> {
    (0..len)
        .map(|i| env.mem.read(Ptr::<u8, false>::from_bits(ptr + i)))
        .collect()
}

fn read_cstr(env: &Environment, ptr: u32) -> Vec<u8> {
    if ptr == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut off: GuestUSize = 0;
    loop {
        let b: u8 = env.mem.read(Ptr::<u8, false>::from_bits(ptr + off));
        if b == 0 {
            break;
        }
        out.push(b);
        off += 1;
    }
    out
}

fn read_cstr_str(env: &Environment, ptr: u32) -> String {
    String::from_utf8_lossy(&read_cstr(env, ptr)).into_owned()
}

fn write_xml_str(env: &mut Environment, s: &[u8]) -> u32 {
    let len = s.len() as GuestUSize;
    let ptr: MutPtr<u8> = env.mem.alloc(len + 1).cast();
    for (i, &b) in s.iter().enumerate() {
        env.mem.write(ptr + (i as GuestUSize), b);
    }
    env.mem.write(ptr + len, 0u8);
    ptr.to_bits()
}

fn write_empty_str(env: &mut Environment) -> u32 {
    write_xml_str(env, b"")
}

// ============================================================
// Minimal XML parser
// ============================================================

fn xml_decode_entities(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'&' {
            out.push(s[i]);
            i += 1;
            continue;
        }
        let rest = &s[i + 1..];
        if let Some(semi) = rest.iter().position(|&c| c == b';') {
            let ent = &rest[..semi];
            let rep: Option<&[u8]> = match ent {
                b"amp" => Some(b"&"),
                b"lt" => Some(b"<"),
                b"gt" => Some(b">"),
                b"apos" => Some(b"'"),
                b"quot" => Some(b"\""),
                _ => None,
            };
            if let Some(r) = rep {
                out.extend_from_slice(r);
                i += 1 + semi + 1;
                continue;
            }
            if ent.starts_with(b"#") {
                let digits = &ent[1..];
                let cp = if digits.starts_with(b"x") || digits.starts_with(b"X")
                {
                    u32::from_str_radix(
                        std::str::from_utf8(&digits[1..]).unwrap_or(""),
                        16,
                    )
                    .ok()
                } else {
                    std::str::from_utf8(digits)
                        .ok()
                        .and_then(|s| s.parse().ok())
                };
                if let Some(c) = cp.and_then(char::from_u32) {
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    i += 1 + semi + 1;
                    continue;
                }
            }
        }
        out.push(b'&');
        i += 1;
    }
    out
}

fn xml_is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

fn xml_trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|&b| !xml_is_ws(b)).unwrap_or(s.len());
    let z = s
        .iter()
        .rposition(|&b| !xml_is_ws(b))
        .map(|p| p + 1)
        .unwrap_or(0);
    if z > a {
        &s[a..z]
    } else {
        b""
    }
}

/// Parse raw XML bytes into a flat event list suitable for
/// both the TextReader API and the DOM builder.
fn xml_parse_events(data: &[u8]) -> Vec<ReaderEvent> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut i = 0;

    while i < data.len() {
        if data[i] != b'<' {
            let start = i;
            while i < data.len() && data[i] != b'<' {
                i += 1;
            }
            let text = xml_decode_entities(&data[start..i]);
            if !xml_trim(&text).is_empty() {
                out.push(ReaderEvent::Text {
                    value: text,
                    depth,
                });
            }
            continue;
        }
        i += 1;
        if i >= data.len() {
            break;
        }

        // Processing instruction / XML declaration
        if data[i] == b'?' {
            while i + 1 < data.len()
                && !(data[i] == b'?' && data[i + 1] == b'>')
            {
                i += 1;
            }
            i += 2;
            continue;
        }
        // Comment <!-- ... -->
        if data[i..].starts_with(b"!--") {
            i += 3;
            while i + 2 < data.len()
                && !(data[i] == b'-'
                    && data[i + 1] == b'-'
                    && data[i + 2] == b'>')
            {
                i += 1;
            }
            i += 3;
            continue;
        }
        // DOCTYPE
        if data[i..].starts_with(b"!DOCTYPE")
            || data[i..].starts_with(b"!doctype")
        {
            while i < data.len() && data[i] != b'>' {
                i += 1;
            }
            if i < data.len() {
                i += 1;
            }
            continue;
        }
        // CDATA section <![CDATA[ ... ]]>
        if data[i..].starts_with(b"![CDATA[") {
            i += 8;
            let start = i;
            while i + 2 < data.len()
                && !(data[i] == b']'
                    && data[i + 1] == b']'
                    && data[i + 2] == b'>')
            {
                i += 1;
            }
            let cdata = data[start..i].to_vec();
            if !xml_trim(&cdata).is_empty() {
                out.push(ReaderEvent::Text {
                    value: cdata,
                    depth,
                });
            }
            i += 3;
            continue;
        }
        // End element </tag>
        if data[i] == b'/' {
            i += 1;
            let start = i;
            while i < data.len() && data[i] != b'>' {
                i += 1;
            }
            let name = xml_trim(&data[start..i]).to_vec();
            if i < data.len() {
                i += 1;
            }
            depth -= 1;
            out.push(ReaderEvent::End { name, depth });
            continue;
        }
        // Start element <tag ...>
        let ns = i;
        while i < data.len()
            && !xml_is_ws(data[i])
            && data[i] != b'>'
            && data[i] != b'/'
        {
            i += 1;
        }
        let tag = data[ns..i].to_vec();

        // Attributes
        let mut attrs: Vec<XmlAttr> = Vec::new();
        loop {
            while i < data.len() && xml_is_ws(data[i]) {
                i += 1;
            }
            if i >= data.len()
                || data[i] == b'>'
                || (data[i] == b'/'
                    && i + 1 < data.len()
                    && data[i + 1] == b'>')
            {
                break;
            }
            let ans = i;
            while i < data.len()
                && !xml_is_ws(data[i])
                && data[i] != b'='
                && data[i] != b'>'
                && data[i] != b'/'
            {
                i += 1;
            }
            let aname = data[ans..i].to_vec();
            while i < data.len() && xml_is_ws(data[i]) {
                i += 1;
            }
            if i >= data.len() || data[i] != b'=' {
                if !aname.is_empty() {
                    attrs.push(XmlAttr {
                        name: aname,
                        value: Vec::new(),
                    });
                }
                continue;
            }
            i += 1; // skip '='
            while i < data.len() && xml_is_ws(data[i]) {
                i += 1;
            }
            let q = if i < data.len() { data[i] } else { b'"' };
            if q == b'"' || q == b'\'' {
                i += 1;
                let avs = i;
                while i < data.len() && data[i] != q {
                    i += 1;
                }
                let val = xml_decode_entities(&data[avs..i]);
                if i < data.len() {
                    i += 1;
                }
                attrs.push(XmlAttr { name: aname, value: val });
            }
        }

        let self_close = i < data.len() && data[i] == b'/';
        if self_close {
            i += 1;
        }
        if i < data.len() && data[i] == b'>' {
            i += 1;
        }

        out.push(ReaderEvent::Start {
            name: tag.clone(),
            attrs: attrs.clone(),
            depth,
            is_empty: self_close,
        });
        if self_close {
            out.push(ReaderEvent::End {
                name: tag,
                depth,
            });
        } else {
            depth += 1;
        }
    }

    out
}

// ============================================================
// DOM builder
// ============================================================

/// Parse XML bytes into a DOM tree stored in XmlState.
/// Returns the doc handle, or 0 on failure.
fn build_doc(state: &mut XmlState, data: &[u8]) -> Option<u32> {
    let events = xml_parse_events(data);
    if events.is_empty() {
        return None;
    }

    let doc_h = state.alloc();

    // Temporary tree built before sibling links are known.
    struct Tmp {
        name: Vec<u8>,
        content: Vec<u8>,
        attrs: Vec<XmlAttr>,
        parent: u32,
        children: Vec<u32>,
    }
    let mut pool: HashMap<u32, Tmp> = HashMap::new();
    let mut stk: Vec<u32> = Vec::new();
    let mut root: Option<u32> = None;

    for ev in &events {
        match ev {
            ReaderEvent::Start {
                name,
                attrs,
                is_empty,
                ..
            } => {
                let nh = state.alloc();
                let ph = stk.last().copied().unwrap_or(0);
                if ph != 0 {
                    if let Some(p) = pool.get_mut(&ph) {
                        p.children.push(nh);
                    }
                }
                pool.insert(
                    nh,
                    Tmp {
                        name: name.clone(),
                        content: Vec::new(),
                        attrs: attrs.clone(),
                        parent: ph,
                        children: Vec::new(),
                    },
                );
                if root.is_none() {
                    root = Some(nh);
                }
                if !is_empty {
                    stk.push(nh);
                }
            }
            ReaderEvent::End { .. } => {
                stk.pop();
            }
            ReaderEvent::Text { value, .. } => {
                if let Some(&ph) = stk.last() {
                    if let Some(p) = pool.get_mut(&ph) {
                        p.content.extend_from_slice(value);
                    }
                }
            }
        }
    }

    // Convert Tmp nodes to XmlNodeData with sibling links.
    for (&nh, tmp) in &pool {
        let fc = tmp.children.first().copied().unwrap_or(0);
        let lc = tmp.children.last().copied().unwrap_or(0);
        state.nodes.insert(
            nh,
            XmlNodeData {
                name: tmp.name.clone(),
                content: tmp.content.clone(),
                attrs: tmp.attrs.clone(),
                parent: tmp.parent,
                first_child: fc,
                last_child: lc,
                next_sib: 0,
                prev_sib: 0,
                doc_handle: doc_h,
            },
        );
    }

    // Fix prev_sib / next_sib for every parent's child list.
    for (_, tmp) in &pool {
        let ch = &tmp.children;
        for i in 0..ch.len() {
            let prev = if i > 0 { ch[i - 1] } else { 0 };
            let next = if i + 1 < ch.len() { ch[i + 1] } else { 0 };
            if let Some(nd) = state.nodes.get_mut(&ch[i]) {
                nd.prev_sib = prev;
                nd.next_sib = next;
            }
        }
    }

    let root_h = root?;
    state.docs.insert(doc_h, XmlDocData { root: root_h });
    Some(doc_h)
}

fn read_xml_file(env: &mut Environment, path: &str) -> Vec<u8> {
    env.fs.read(GuestPath::new(path)).unwrap_or_default()
}

// ============================================================
// TextReader helpers
// ============================================================

fn reader_current(rd: &XmlReaderData) -> Option<&ReaderEvent> {
    if rd.pos < 0 || rd.pos as usize >= rd.events.len() {
        None
    } else {
        Some(&rd.events[rd.pos as usize])
    }
}

fn event_node_type(ev: &ReaderEvent) -> i32 {
    match ev {
        ReaderEvent::Start { .. } => XML_READER_TYPE_ELEMENT,
        ReaderEvent::End { .. } => XML_READER_TYPE_END_ELEMENT,
        ReaderEvent::Text { .. } => XML_READER_TYPE_TEXT,
    }
}

fn event_depth(ev: &ReaderEvent) -> i32 {
    match ev {
        ReaderEvent::Start { depth, .. }
        | ReaderEvent::End { depth, .. }
        | ReaderEvent::Text { depth, .. } => *depth,
    }
}

fn event_name(ev: &ReaderEvent) -> &[u8] {
    match ev {
        ReaderEvent::Start { name, .. } | ReaderEvent::End { name, .. } => {
            name
        }
        ReaderEvent::Text { .. } => b"#text",
    }
}

fn event_value(ev: &ReaderEvent) -> &[u8] {
    match ev {
        ReaderEvent::Text { value, .. } => value,
        _ => b"",
    }
}

fn event_attrs(ev: &ReaderEvent) -> &[XmlAttr] {
    match ev {
        ReaderEvent::Start { attrs, .. } => attrs,
        _ => &[],
    }
}

fn event_is_empty(ev: &ReaderEvent) -> bool {
    matches!(ev, ReaderEvent::Start { is_empty: true, .. })
}

// ============================================================
// xmlParserCtxt stubs
// ============================================================

#[allow(non_snake_case)]
fn xmlNewParserCtxt(_env: &mut Environment) -> u32 {
    with_xml(|s| s.alloc())
}

#[allow(non_snake_case)]
fn xmlClearParserCtxt(_env: &mut Environment) {}

#[allow(non_snake_case)]
fn xmlFreeParserCtxt(_env: &mut Environment, ctxt: u32) {
    with_xml(|s| s.release(ctxt));
}

#[allow(non_snake_case)]
fn xmlCtxtGetLastError(_env: &mut Environment, _ctxt: u32) -> u32 {
    0
}

#[allow(non_snake_case)]
fn xmlCleanupParser(_env: &mut Environment) {}

// ============================================================
// Document parsing
// ============================================================

#[allow(non_snake_case)]
fn xmlCtxtReadMemory(
    env: &mut Environment,
    _ctxt: u32,
    buf: u32,
    sz: u32,
    _url: u32,
    _enc: u32,
    _opt: u32,
) -> u32 {
    let data = read_guest_bytes(env, buf, sz);
    with_xml(|s| build_doc(s, &data).unwrap_or(0))
}

#[allow(non_snake_case)]
fn xmlReadFile(
    env: &mut Environment,
    filename: u32,
    _encoding: u32,
    _options: u32,
) -> u32 {
    let path = read_cstr_str(env, filename);
    log!("xmlReadFile(\"{}\")", path);
    let data = read_xml_file(env, &path);
    with_xml(|s| build_doc(s, &data).unwrap_or(0))
}

#[allow(non_snake_case)]
fn xmlReadMemory(
    env: &mut Environment,
    buffer: u32,
    size: u32,
    _url: u32,
    _encoding: u32,
    _options: u32,
) -> u32 {
    let data = read_guest_bytes(env, buffer, size);
    with_xml(|s| build_doc(s, &data).unwrap_or(0))
}

#[allow(non_snake_case)]
fn xmlParseMemory(env: &mut Environment, buffer: u32, size: u32) -> u32 {
    let data = read_guest_bytes(env, buffer, size);
    with_xml(|s| build_doc(s, &data).unwrap_or(0))
}

#[allow(non_snake_case)]
fn xmlFreeDoc(_env: &mut Environment, doc: u32) {
    with_xml(|s| {
        s.docs.remove(&doc);
        let to_rm: Vec<u32> = s
            .nodes
            .iter()
            .filter(|(_, n)| n.doc_handle == doc)
            .map(|(h, _)| *h)
            .collect();
        for h in to_rm {
            s.nodes.remove(&h);
            s.handles.remove(&h);
        }
        s.handles.remove(&doc);
    });
}

#[allow(non_snake_case)]
fn xmlFree(_env: &mut Environment, ptr: u32) {
    // Release host-side handle; guest xmlChar* strings are
    // left as an acceptable leak (no env.mem.free() needed).
    with_xml(|s| {
        if s.is_handle(ptr) {
            s.release(ptr);
        }
    });
}

// ============================================================
// DOM traversal
// ============================================================

#[allow(non_snake_case)]
fn xmlDocGetRootElement(_env: &mut Environment, doc: u32) -> u32 {
    with_xml(|s| s.docs.get(&doc).map(|d| d.root).unwrap_or(0))
}

#[allow(non_snake_case)]
fn xmlChildrenNode(_env: &mut Environment, node: u32) -> u32 {
    with_xml(|s| {
        s.nodes.get(&node).map(|n| n.first_child).unwrap_or(0)
    })
}

#[allow(non_snake_case)]
fn xmlGetLastChild(_env: &mut Environment, node: u32) -> u32 {
    with_xml(|s| {
        s.nodes.get(&node).map(|n| n.last_child).unwrap_or(0)
    })
}

/// Walk next_sib links until an element node is found.
#[allow(non_snake_case)]
fn xmlNextElementSibling(_env: &mut Environment, node: u32) -> u32 {
    with_xml(|s| {
        let mut cur =
            s.nodes.get(&node).map(|n| n.next_sib).unwrap_or(0);
        while cur != 0 {
            match s.nodes.get(&cur) {
                Some(n) if !n.name.is_empty() => return cur,
                Some(n) => cur = n.next_sib,
                None => break,
            }
        }
        0
    })
}

/// Walk first_child / next_sib links until an element is found.
#[allow(non_snake_case)]
fn xmlFirstElementChild(_env: &mut Environment, node: u32) -> u32 {
    with_xml(|s| {
        let mut cur =
            s.nodes.get(&node).map(|n| n.first_child).unwrap_or(0);
        while cur != 0 {
            match s.nodes.get(&cur) {
                Some(n) if !n.name.is_empty() => return cur,
                Some(n) => cur = n.next_sib,
                None => break,
            }
        }
        0
    })
}

#[allow(non_snake_case)]
fn xmlNodeListGetString(
    env: &mut Environment,
    _doc: u32,
    list: u32,
    _inln: u32,
) -> u32 {
    let content = with_xml(|s| {
        s.nodes
            .get(&list)
            .map(|n| n.content.clone())
            .unwrap_or_default()
    });
    write_xml_str(env, &content)
}

#[allow(non_snake_case)]
fn xmlGetProp(env: &mut Environment, node: u32, name: u32) -> u32 {
    let attr_name = read_cstr(env, name);
    let val = with_xml(|s| {
        s.nodes.get(&node).and_then(|n| {
            n.attrs
                .iter()
                .find(|a| a.name == attr_name)
                .map(|a| a.value.clone())
        })
    });
    match val {
        Some(v) => write_xml_str(env, &v),
        None => 0,
    }
}

#[allow(non_snake_case)]
fn xmlHasProp(env: &mut Environment, node: u32, name: u32) -> u32 {
    let attr_name = read_cstr(env, name);
    with_xml(|s| {
        let found = s
            .nodes
            .get(&node)
            .map(|n| n.attrs.iter().any(|a| a.name == attr_name))
            .unwrap_or(false);
        u32::from(found)
    })
}

#[allow(non_snake_case)]
fn xmlStrcmp(env: &mut Environment, s1: u32, s2: u32) -> i32 {
    let a = read_cstr(env, s1);
    let b = read_cstr(env, s2);
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

#[allow(non_snake_case)]
fn xmlNodeGetContent(env: &mut Environment, node: u32) -> u32 {
    let content = with_xml(|s| {
        s.nodes
            .get(&node)
            .map(|n| n.content.clone())
            .unwrap_or_default()
    });
    write_xml_str(env, &content)
}

// ============================================================
// String utilities
// ============================================================

#[allow(non_snake_case)]
fn xmlStrdup(env: &mut Environment, src: u32) -> u32 {
    if src == 0 {
        return 0;
    }
    let bytes = read_cstr(env, src);
    write_xml_str(env, &bytes)
}

#[allow(non_snake_case)]
fn xmlStrlen(env: &mut Environment, str_ptr: u32) -> i32 {
    if str_ptr == 0 {
        return 0;
    }
    read_cstr(env, str_ptr).len() as i32
}

#[allow(non_snake_case)]
fn xmlStrsub(env: &mut Environment, str_ptr: u32, start: u32, len: u32) -> u32 {
    if str_ptr == 0 {
        return 0;
    }
    let bytes = read_cstr(env, str_ptr);
    let s = start as usize;
    let l = len as usize;
    let slice = if s < bytes.len() {
        &bytes[s..(s + l).min(bytes.len())]
    } else {
        &[]
    };
    write_xml_str(env, slice)
}

// ============================================================
// TextReader API
// ============================================================

#[allow(non_snake_case)]
fn xmlReaderForFile(
    env: &mut Environment,
    filename: u32,
    _encoding: u32,
    _options: u32,
) -> u32 {
    let path = read_cstr_str(env, filename);
    log!("xmlReaderForFile(\"{}\")", path);
    let data = read_xml_file(env, &path);
    if data.is_empty() {
        log!("xmlReaderForFile: file not found or empty: {}", path);
        return 0;
    }
    let events = xml_parse_events(&data);
    with_xml(|s| {
        let h = s.alloc();
        s.readers.insert(h, XmlReaderData { events, pos: -1 });
        h
    })
}

#[allow(non_snake_case)]
fn xmlFreeTextReader(_env: &mut Environment, reader: u32) {
    with_xml(|s| s.release(reader));
}

/// Advance to next event; 1 = node available, 0 = EOF, -1 = err.
#[allow(non_snake_case)]
fn xmlTextReaderRead(_env: &mut Environment, reader: u32) -> i32 {
    with_xml(|s| match s.readers.get_mut(&reader) {
        Some(rd) => {
            rd.pos += 1;
            if (rd.pos as usize) < rd.events.len() {
                1
            } else {
                0
            }
        }
        None => -1,
    })
}

#[allow(non_snake_case)]
fn xmlTextReaderNodeType(_env: &mut Environment, reader: u32) -> i32 {
    with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .map(event_node_type)
            .unwrap_or(0)
    })
}

#[allow(non_snake_case)]
fn xmlTextReaderConstName(env: &mut Environment, reader: u32) -> u32 {
    let name = with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .map(|ev| event_name(ev).to_vec())
            .unwrap_or_default()
    });
    write_xml_str(env, &name)
}

#[allow(non_snake_case)]
fn xmlTextReaderName(env: &mut Environment, reader: u32) -> u32 {
    xmlTextReaderConstName(env, reader)
}

#[allow(non_snake_case)]
fn xmlTextReaderConstValue(env: &mut Environment, reader: u32) -> u32 {
    let val = with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .map(|ev| event_value(ev).to_vec())
            .unwrap_or_default()
    });
    write_xml_str(env, &val)
}

#[allow(non_snake_case)]
fn xmlTextReaderValue(env: &mut Environment, reader: u32) -> u32 {
    xmlTextReaderConstValue(env, reader)
}

#[allow(non_snake_case)]
fn xmlTextReaderReadInnerXml(env: &mut Environment, _reader: u32) -> u32 {
    write_empty_str(env)
}

#[allow(non_snake_case)]
fn xmlTextReaderReadOuterXml(env: &mut Environment, _reader: u32) -> u32 {
    write_empty_str(env)
}

#[allow(non_snake_case)]
fn xmlTextReaderGetAttribute(
    env: &mut Environment,
    reader: u32,
    name: u32,
) -> u32 {
    let attr_name = read_cstr(env, name);
    let val = with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .and_then(|ev| {
                event_attrs(ev)
                    .iter()
                    .find(|a| a.name == attr_name)
                    .map(|a| a.value.clone())
            })
    });
    match val {
        Some(v) => write_xml_str(env, &v),
        None => 0,
    }
}

#[allow(non_snake_case)]
fn xmlTextReaderAttributeCount(_env: &mut Environment, reader: u32) -> i32 {
    with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .map(|ev| event_attrs(ev).len() as i32)
            .unwrap_or(0)
    })
}

#[allow(non_snake_case)]
fn xmlTextReaderDepth(_env: &mut Environment, reader: u32) -> i32 {
    with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .map(event_depth)
            .unwrap_or(0)
    })
}

#[allow(non_snake_case)]
fn xmlTextReaderHasAttributes(_env: &mut Environment, reader: u32) -> i32 {
    with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .map(|ev| i32::from(!event_attrs(ev).is_empty()))
            .unwrap_or(0)
    })
}

#[allow(non_snake_case)]
fn xmlTextReaderIsEmptyElement(_env: &mut Environment, reader: u32) -> i32 {
    with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .map(|ev| i32::from(event_is_empty(ev)))
            .unwrap_or(0)
    })
}

/// Skip past the current node and all its descendants; land on
/// the next element at the same or outer depth.
#[allow(non_snake_case)]
fn xmlTextReaderNext(_env: &mut Environment, reader: u32) -> i32 {
    with_xml(|s| {
        let rd = match s.readers.get_mut(&reader) {
            Some(r) => r,
            None => return -1,
        };
        let cur_depth = reader_current(rd).map(event_depth).unwrap_or(0);
        loop {
            rd.pos += 1;
            if rd.pos as usize >= rd.events.len() {
                return 0;
            }
            let ev = &rd.events[rd.pos as usize];
            if matches!(ev, ReaderEvent::Start { .. })
                && event_depth(ev) <= cur_depth
            {
                return 1;
            }
        }
    })
}

/// Move to the next sibling element at the same depth.
#[allow(non_snake_case)]
fn xmlTextReaderNextSibling(_env: &mut Environment, reader: u32) -> i32 {
    with_xml(|s| {
        let rd = match s.readers.get_mut(&reader) {
            Some(r) => r,
            None => return -1,
        };
        let cur_depth = reader_current(rd).map(event_depth).unwrap_or(0);
        loop {
            rd.pos += 1;
            if rd.pos as usize >= rd.events.len() {
                return 0;
            }
            let ev = &rd.events[rd.pos as usize];
            let d = event_depth(ev);
            if d < cur_depth {
                return 0;
            }
            if d == cur_depth && matches!(ev, ReaderEvent::Start { .. }) {
                return 1;
            }
        }
    })
}

/// Local name = tag name stripped of any "prefix:" part.
#[allow(non_snake_case)]
fn xmlTextReaderLocalName(env: &mut Environment, reader: u32) -> u32 {
    let local = with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .map(|ev| {
                let n = event_name(ev);
                match n.iter().position(|&b| b == b':') {
                    Some(pos) => n[pos + 1..].to_vec(),
                    None => n.to_vec(),
                }
            })
            .unwrap_or_default()
    });
    write_xml_str(env, &local)
}

/// Namespace prefix ("" for un-prefixed names).
#[allow(non_snake_case)]
fn xmlTextReaderPrefix(env: &mut Environment, reader: u32) -> u32 {
    let prefix = with_xml(|s| {
        s.readers
            .get(&reader)
            .and_then(|rd| reader_current(rd))
            .map(|ev| {
                let n = event_name(ev);
                match n.iter().position(|&b| b == b':') {
                    Some(pos) => n[..pos].to_vec(),
                    None => Vec::new(),
                }
            })
            .unwrap_or_default()
    });
    write_xml_str(env, &prefix)
}

#[allow(non_snake_case)]
fn xmlTextReaderNamespaceUri(env: &mut Environment, _reader: u32) -> u32 {
    write_empty_str(env)
}

#[allow(non_snake_case)]
fn xmlNewDoc(env: &mut Environment, version_ptr: u32) -> u32 {
    // Читаем версию (обычно "1.0"), но для нашей внутренней структуры 
    // она пока не требуется. Просто логируем для отладки.
    let version = read_cstr_str(env, version_ptr);
    log!("xmlNewDoc(version: {:?})", version);

    with_xml(|s| {
        // Выделяем новый безопасный хэндл
        let doc_h = s.alloc();
        // Создаем пустой документ без корневого элемента (root: 0)
        s.docs.insert(doc_h, XmlDocData { root: 0 });
        
        doc_h
    })
}

#[allow(non_snake_case)]
fn xmlNewNode(env: &mut Environment, _ns: u32, name: u32) -> u32 {
    // Читаем имя создаваемого тега из памяти гостя
    let name_bytes = read_cstr(env, name);
    log!("xmlNewNode(name: {:?})", String::from_utf8_lossy(&name_bytes));

    with_xml(|s| {
        let node_h = s.alloc();
        s.nodes.insert(
            node_h,
            XmlNodeData {
                name: name_bytes,
                content: Vec::new(),
                attrs: Vec::new(),
                parent: 0,
                first_child: 0,
                last_child: 0,
                next_sib: 0,
                prev_sib: 0,
                doc_handle: 0, // Пока нода не привязана к документу
            },
        );
        node_h
    })
}

#[allow(non_snake_case)]
fn xmlDocSetRootElement(_env: &mut Environment, doc: u32, root: u32) -> u32 {
    with_xml(|s| {
        let mut old_root = 0;
        
        // Привязываем корень к документу
        if let Some(d) = s.docs.get_mut(&doc) {
            old_root = d.root;
            d.root = root;
        }
        
        // Обновляем ссылку на документ внутри самой ноды
        if root != 0 {
            if let Some(n) = s.nodes.get_mut(&root) {
                n.doc_handle = doc;
            }
        }
        
        // По стандарту libxml2 возвращается старый корень (или 0)
        old_root 
    })
}

// ============================================================
// XML Serialization & Writing
// ============================================================

fn escape_xml(text: &[u8], is_attr: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in text {
        match b {
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'&' => out.extend_from_slice(b"&amp;"),
            b'"' if is_attr => out.extend_from_slice(b"&quot;"),
            b'\'' if is_attr => out.extend_from_slice(b"&apos;"),
            _ => out.push(b),
        }
    }
    out
}

fn serialize_node(state: &XmlState, node_h: u32, out: &mut Vec<u8>) {
    if node_h == 0 {
        return;
    }
    let node = if let Some(n) = state.nodes.get(&node_h) {
        n.clone()
    } else {
        return;
    };

    if node.name.is_empty() || node.name == b"#text" {
        out.extend_from_slice(&escape_xml(&node.content, false));
    } else {
        out.push(b'<');
        out.extend_from_slice(&node.name);

        for attr in &node.attrs {
            out.push(b' ');
            out.extend_from_slice(&attr.name);
            out.extend_from_slice(b"=\"");
            out.extend_from_slice(&escape_xml(&attr.value, true));
            out.push(b'"');
        }

        if node.first_child == 0 && node.content.is_empty() {
            out.extend_from_slice(b"/>\n");
        } else {
            out.extend_from_slice(b">\n");
            if !node.content.is_empty() {
                out.extend_from_slice(&escape_xml(&node.content, false));
            }

            let mut curr_child = node.first_child;
            while curr_child != 0 {
                serialize_node(state, curr_child, out);
                curr_child = state.nodes.get(&curr_child).map(|n| n.next_sib).unwrap_or(0);
            }

            out.extend_from_slice(b"</");
            out.extend_from_slice(&node.name);
            out.extend_from_slice(b">\n");
        }
    }
}

#[allow(non_snake_case)]
fn xmlSaveFile(env: &mut Environment, filename_ptr: u32, doc: u32) -> i32 {
    let filename = read_cstr_str(env, filename_ptr);
    log!("xmlSaveFile(filename: {:?}, doc: {})", filename, doc);

    let mut out = Vec::new();
    out.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

    let success = with_xml(|s| {
        if let Some(d) = s.docs.get(&doc) {
            serialize_node(s, d.root, &mut out);
            true
        } else {
            false
        }
    });

    if !success {
        return -1;
    }

    // Пробуем сохранить файл через встроенную ФС
    match env.fs.write(GuestPath::new(&filename), &out) {
        Ok(_) => out.len() as i32,
        Err(e) => {
            // ИСПРАВЛЕНО: используем {:?} для вывода (), если запись не удалась
            log!("xmlSaveFile: failed to write using env.fs.write: {:?}", e);
            
            // Фолбэк для Android
            let safe_name = filename.replace("/", "_");
            let fallback_path = format!("/storage/emulated/0/Android/data/org.hyperhle.android.unofficial/files/{}", safe_name);
            log!("xmlSaveFile: trying fallback path: {}", fallback_path);
            
            match std::fs::write(&fallback_path, &out) {
                Ok(_) => out.len() as i32,
                Err(e2) => {
                    log!("xmlSaveFile: fallback also failed: {:?}", e2);
                    -1
                }
            }
        }
    }
}

// ============================================================
// DOM Modification Helpers
// ============================================================

#[allow(non_snake_case)]
fn xmlAddChild(_env: &mut Environment, parent: u32, cur: u32) -> u32 {
    with_xml(|s| {
        if !s.is_handle(parent) || !s.is_handle(cur) {
            return 0;
        }
        if let Some(p) = s.nodes.get_mut(&parent) {
            let last = p.last_child;
            p.last_child = cur;
            if p.first_child == 0 {
                p.first_child = cur;
            }
            let doc_h = p.doc_handle;
            
            if let Some(c) = s.nodes.get_mut(&cur) {
                c.parent = parent;
                c.prev_sib = last;
                c.next_sib = 0;
                c.doc_handle = doc_h;
            }
            
            if last != 0 {
                if let Some(l) = s.nodes.get_mut(&last) {
                    l.next_sib = cur;
                }
            }
        }
        cur
    })
}

#[allow(non_snake_case)]
fn xmlNewProp(env: &mut Environment, node: u32, name: u32, value: u32) -> u32 {
    let name_bytes = read_cstr(env, name);
    let value_bytes = read_cstr(env, value);
    
    with_xml(|s| {
        if let Some(n) = s.nodes.get_mut(&node) {
            n.attrs.push(XmlAttr {
                name: name_bytes,
                value: value_bytes,
            });
        }
        node // возвращаем фейковый указатель, чтобы игра не упала с NULL check
    })
}

#[allow(non_snake_case)]
fn xmlSetProp(env: &mut Environment, node: u32, name: u32, value: u32) -> u32 {
    xmlNewProp(env, node, name, value)
}

#[allow(non_snake_case)]
fn xmlNewText(env: &mut Environment, content: u32) -> u32 {
    let content_bytes = read_cstr(env, content);
    with_xml(|s| {
        let node_h = s.alloc();
        s.nodes.insert(
            node_h,
            XmlNodeData {
                name: b"#text".to_vec(),
                content: content_bytes,
                attrs: Vec::new(),
                parent: 0,
                first_child: 0,
                last_child: 0,
                next_sib: 0,
                prev_sib: 0,
                doc_handle: 0,
            },
        );
        node_h
    })
}

#[allow(non_snake_case)]
fn xmlNewChild(env: &mut Environment, parent: u32, ns: u32, name: u32, content: u32) -> u32 {
    let node_h = xmlNewNode(env, ns, name);
    if content != 0 {
        let content_bytes = read_cstr(env, content);
        with_xml(|s| {
            if let Some(n) = s.nodes.get_mut(&node_h) {
                n.content = content_bytes;
            }
        });
    }
    if parent != 0 {
        xmlAddChild(env, parent, node_h);
    }
    node_h
}

#[allow(non_snake_case)]
fn xmlNodeSetContent(env: &mut Environment, node: u32, content: u32) {
    let content_bytes = read_cstr(env, content);
    with_xml(|s| {
        if let Some(n) = s.nodes.get_mut(&node) {
            n.content = content_bytes;
        }
    });
}
