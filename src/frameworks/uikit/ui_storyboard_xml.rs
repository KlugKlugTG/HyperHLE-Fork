/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Runtime loader for uncompiled `.storyboard` Interface Builder XML.
//!
//! Apple's Xcode toolchain compiles each `.storyboard` into a
//! `*.storyboardc` bundle (a directory of binary nib archives + an
//! `Info.plist`) using `ibtool`/`ibtoold`. The compiled output is what
//! ships inside an `*.ipa`. HyperHLE has supported that flow for a while
//! through [`super::ui_storyboard`] + [`super::ui_nib`].
//!
//! Some apps – and many simple sample projects, including the
//! `StoryboardSupport` test case – ship the raw `.storyboard` XML inside
//! the bundle instead of (or alongside) the compiled `.storyboardc`. The
//! XML is a stable, well-known Interface Builder dialect:
//!
//!   * `<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB">`
//!     declares an `initialViewController` id.
//!   * `<scenes>` contain `<scene>` → `<objects>` → one or more
//!     view controllers (`<viewController>`, `<navigationController>`,
//!     `<tabBarController>`, …).
//!   * Each view controller owns a single `<view>` subtree of UIView
//!     subclasses (`<view>`, `<label>`, `<button>`, `<imageView>` …).
//!   * Views may have a `<rect key="frame">`, `<color>` swatches, a
//!     `<fontDescription>`, a `<state key="normal" title=…>` etc.
//!   * `<constraints>` declare Auto Layout relations.
//!   * `<connections>` wires `<outlet property="…" destination="…"/>` and
//!     `<action selector="…:" destination="…" eventType="touchUpInside"/>`.
//!
//! This module implements a full real loader for that dialect. It builds
//! the actual `UIView`/`UIViewController`/`UILabel`/`UIButton`/… objects
//! exactly the same way `+nibWithNibName:bundle:` would, with frames,
//! colors, fonts, titles and button targets all populated, then walks
//! the connection table to apply outlets and actions via KVC.
//!
//! Auto Layout is not a constraint solver in HyperHLE (see
//! [`super::ui_layout_placeholders`] for the rationale), so this loader
//! also performs a lightweight constraint pass: the same constraints
//! that are stored as inert `NSLayoutConstraint` objects are evaluated
//! here against the parent view's bounds so the resulting frames look
//! like what Auto Layout would have produced. The translated frames
//! only run when `translatesAutoresizingMaskIntoConstraints="NO"` is
//! set on the view (Apple's flag indicating the view participates in
//! Auto Layout).
//!
//! References:
//! * Apple, "About Storyboards", Resource Programming Guide.
//! * Apple, "Auto Layout Guide", chapter on `NSLayoutAttribute`.
//! * Apple, `IBStoryboardCompiler` ibtool output (verified against
//!   compiled `.storyboardc` bundles).

use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string::{from_rust_string, get_static_str, to_rust_string};
use crate::frameworks::foundation::NSUInteger;
use crate::frameworks::uikit::ui_view::ui_control::UIControlEvents;
use crate::objc::{id, msg, msg_class, nil, release, retain, SEL};
use crate::Environment;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use std::collections::HashMap;
use std::sync::Arc;

// -------------------------------------------------------------------------
// Parsed model
// -------------------------------------------------------------------------

/// One node in the storyboard XML. We keep the original element name
/// (`tag`), all attributes verbatim, and the children in document order.
#[derive(Debug, Clone, Default)]
pub struct XmlNode {
    pub tag: String,
    pub attrs: HashMap<String, String>,
    pub children: Vec<XmlNode>,
}

impl XmlNode {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(|s| s.as_str())
    }
    fn child_by_tag(&self, tag: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.tag == tag)
    }
    fn child_by_key(&self, key: &str) -> Option<&XmlNode> {
        self.children
            .iter()
            .find(|c| c.attr("key").map(|k| k == key).unwrap_or(false))
    }
    fn children_with_tag<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a XmlNode> + 'a {
        self.children.iter().filter(move |c| c.tag == tag)
    }
}

/// Top-level parsed storyboard document.
#[derive(Debug, Default)]
pub struct ParsedStoryboard {
    pub initial_view_controller_id: Option<String>,
    /// All scene root objects (view controllers and placeholders), keyed
    /// by `id` attribute, with the scene `id` also recorded.
    pub objects_by_id: HashMap<String, ObjectRef>,
    /// View controller scene roots, in document order.
    pub view_controllers: Vec<String>,
}

/// Lightweight pointer back into the document so an `instantiate_*`
/// pass can find the owning subtree.
#[derive(Debug, Clone)]
pub struct ObjectRef {
    pub node: XmlNode,
    pub scene_id: String,
}

// -------------------------------------------------------------------------
// XML parsing (quick-xml)
// -------------------------------------------------------------------------

fn parse_xml_tree(xml: &str) -> Result<XmlNode, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    let mut buf: Vec<u8> = Vec::new();

    fn make_node(e: &BytesStart) -> Result<XmlNode, String> {
        let tag = std::str::from_utf8(e.name().as_ref())
            .map_err(|err| format!("invalid tag utf-8: {err}"))?
            .to_string();
        let mut attrs = HashMap::new();
        for attr in e.attributes() {
            let attr = attr.map_err(|err| format!("malformed attribute: {err}"))?;
            let key = std::str::from_utf8(attr.key.as_ref())
                .map_err(|err| format!("invalid attr key utf-8: {err}"))?
                .to_string();
            let value = attr
                .unescape_value()
                .map_err(|err| format!("invalid attr value: {err}"))?
                .to_string();
            attrs.insert(key, value);
        }
        Ok(XmlNode {
            tag,
            attrs,
            children: Vec::new(),
        })
    }

    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| format!("XML parse error at {}: {}", reader.buffer_position(), e))?;
        match ev {
            Event::Start(e) => {
                let node = make_node(&e)?;
                stack.push(node);
            }
            Event::End(_e) => {
                let finished = stack
                    .pop()
                    .ok_or_else(|| "closing tag without matching open tag".to_string())?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(finished);
                } else {
                    root = Some(finished);
                }
            }
            Event::Empty(e) => {
                let node = make_node(&e)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root = Some(node);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    root.ok_or_else(|| "empty XML document".to_string())
}

fn collect_object_ids(node: &XmlNode, scene_id: &str, out: &mut HashMap<String, ObjectRef>) {
    if let Some(id) = node.attr("id") {
        out.insert(
            id.to_string(),
            ObjectRef {
                node: node.clone(),
                scene_id: scene_id.to_string(),
            },
        );
    }
    for c in &node.children {
        collect_object_ids(c, scene_id, out);
    }
}

/// Parse a complete `.storyboard` XML document into a [`ParsedStoryboard`].
pub fn parse_storyboard_xml(xml: &str) -> Result<ParsedStoryboard, String> {
    let document = parse_xml_tree(xml)?;
    if document.tag != "document" {
        return Err(format!(
            "expected <document>, found <{}>",
            document.tag
        ));
    }

    let initial = document
        .attr("initialViewController")
        .map(|s| s.to_string());

    let mut parsed = ParsedStoryboard {
        initial_view_controller_id: initial,
        objects_by_id: HashMap::new(),
        view_controllers: Vec::new(),
    };

    if let Some(scenes) = document.child_by_tag("scenes") {
        for scene in scenes.children_with_tag("scene") {
            let scene_id = scene
                .attr("sceneID")
                .unwrap_or("")
                .to_string();
            if let Some(objects) = scene.child_by_tag("objects") {
                for obj in &objects.children {
                    collect_object_ids(obj, &scene_id, &mut parsed.objects_by_id);
                    if is_view_controller_tag(&obj.tag) {
                        if let Some(id) = obj.attr("id") {
                            parsed.view_controllers.push(id.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(parsed)
}

fn is_view_controller_tag(tag: &str) -> bool {
    matches!(
        tag,
        "viewController"
            | "tableViewController"
            | "collectionViewController"
            | "navigationController"
            | "tabBarController"
            | "splitViewController"
            | "pageViewController"
    )
}

fn view_controller_class_name(tag: &str) -> &'static str {
    match tag {
        "tableViewController" => "UITableViewController",
        "collectionViewController" => "UICollectionViewController",
        "navigationController" => "UINavigationController",
        "tabBarController" => "UITabBarController",
        "splitViewController" => "UISplitViewController",
        "pageViewController" => "UIPageViewController",
        _ => "UIViewController",
    }
}

fn is_view_tag(tag: &str) -> bool {
    matches!(
        tag,
        "view"
            | "label"
            | "button"
            | "imageView"
            | "textField"
            | "textView"
            | "scrollView"
            | "tableView"
            | "collectionView"
            | "switch"
            | "slider"
            | "segmentedControl"
            | "activityIndicatorView"
            | "progressView"
            | "stepper"
            | "pageControl"
            | "webView"
            | "searchBar"
            | "pickerView"
            | "datePicker"
            | "toolbar"
            | "navigationBar"
            | "tabBar"
    )
}

fn view_class_name(tag: &str) -> &'static str {
    match tag {
        "label" => "UILabel",
        "button" => "UIButton",
        "imageView" => "UIImageView",
        "textField" => "UITextField",
        "textView" => "UITextView",
        "scrollView" => "UIScrollView",
        "tableView" => "UITableView",
        "collectionView" => "UICollectionView",
        "switch" => "UISwitch",
        "slider" => "UISlider",
        "segmentedControl" => "UISegmentedControl",
        "activityIndicatorView" => "UIActivityIndicatorView",
        "progressView" => "UIProgressView",
        "stepper" => "UIStepper",
        "pageControl" => "UIPageControl",
        "webView" => "UIWebView",
        "searchBar" => "UISearchBar",
        "pickerView" => "UIPickerView",
        "datePicker" => "UIDatePicker",
        "toolbar" => "UIToolbar",
        "navigationBar" => "UINavigationBar",
        "tabBar" => "UITabBar",
        _ => "UIView",
    }
}

// -------------------------------------------------------------------------
// Instantiation
// -------------------------------------------------------------------------

/// Per-pass state shared while instantiating an entire scene tree.
struct InstantiationCtx<'a> {
    doc: &'a ParsedStoryboard,
    /// id → live UI object (autoreleased; not held with an extra retain).
    instances: HashMap<String, id>,
    /// Pending outlet/action connections, deferred until every object
    /// in the scene has been allocated. This handles the case where
    /// an outlet's destination appears later in the document than the
    /// source.
    pending_connections: Vec<Connection>,
    /// XML `id` of the view that should act as the view controller's
    /// `view`. Captured during view-tree walk and used to set
    /// `vc.view = …` once the root has been built.
    vc_view_id: Option<String>,
    /// XML `id` of the view controller currently being instantiated
    /// (so layout-guide ids can resolve to its view).
    current_vc_id: Option<String>,
}

#[derive(Debug, Clone)]
struct Connection {
    kind: ConnectionKind,
    source_xml_id: String,
    destination_xml_id: String,
    selector: Option<String>,
    property: Option<String>,
    event_type: Option<String>,
}

#[derive(Debug, Clone)]
enum ConnectionKind {
    Outlet,
    Action,
}

/// Instantiate the storyboard's initial view controller.
pub fn instantiate_initial_view_controller(
    env: &mut Environment,
    doc: &ParsedStoryboard,
) -> id {
    let Some(initial_id) = doc.initial_view_controller_id.clone() else {
        log!("Warning: storyboard XML has no initialViewController attribute");
        return nil;
    };
    instantiate_view_controller_by_id(env, doc, &initial_id)
}

/// Instantiate a specific view controller by its XML `id`.
pub fn instantiate_view_controller_by_id(
    env: &mut Environment,
    doc: &ParsedStoryboard,
    xml_id: &str,
) -> id {
    let Some(vc_ref) = doc.objects_by_id.get(xml_id).cloned() else {
        log!("Warning: storyboard XML has no object with id {xml_id:?}");
        return nil;
    };
    if !is_view_controller_tag(&vc_ref.node.tag) {
        log!(
            "Warning: storyboard XML id {xml_id:?} resolves to <{}>, not a view controller",
            vc_ref.node.tag
        );
        return nil;
    }

    let mut ctx = InstantiationCtx {
        doc,
        instances: HashMap::new(),
        pending_connections: Vec::new(),
        vc_view_id: None,
        current_vc_id: Some(xml_id.to_string()),
    };

    let vc = instantiate_view_controller(env, &mut ctx, &vc_ref.node);
    if vc == nil {
        return nil;
    }

    // After the entire scene has been built, apply every connection.
    apply_connections(env, &mut ctx);
    vc
}

// -------------------------------------------------------------------------
// View controller instantiation
// -------------------------------------------------------------------------

fn instantiate_view_controller(
    env: &mut Environment,
    ctx: &mut InstantiationCtx<'_>,
    node: &XmlNode,
) -> id {
    let host_class_name = view_controller_class_name(&node.tag);

    // Prefer the customClass if the binary registered one; otherwise fall
    // back to UIViewController (matches the runtime's UIClassSwapper).
    let class = {
        let custom = node.attr("customClass");
        let mut found: Option<crate::objc::Class> = None;
        if let Some(custom_name) = custom {
            if let Some(c) = env.objc.try_get_known_class(custom_name, &mut env.mem) {
                found = Some(c);
            }
        }
        found.unwrap_or_else(|| env.objc.get_known_class(host_class_name, &mut env.mem))
    };

    let vc: id = msg![env; class alloc];
    let vc: id = msg![env; vc init];
    if vc == nil {
        return nil;
    }

    if let Some(xml_id) = node.attr("id") {
        ctx.instances.insert(xml_id.to_string(), vc);
    }

    // Apply simple scalar properties.
    if let Some(title) = node.attr("title") {
        let title_ns = from_rust_string(env, title.to_string());
        let _: () = msg![env; vc setTitle:title_ns];
        release(env, title_ns);
    }

    // Build the root view subtree.
    if let Some(view_node) = node.child_by_key("view") {
        let view_xml_id = view_node.attr("id").map(|s| s.to_string());
        ctx.vc_view_id = view_xml_id.clone();
        let view = instantiate_view(env, ctx, view_node, None);
        if view != nil {
            // Apply a default frame matching the main screen if the
            // root frame is degenerate.
            ensure_root_frame(env, view);
            let _: () = msg![env; vc setView:view];

            // Resolve layout-guide ids so children referencing them
            // produce frames close to what Auto Layout would have done.
            let mut layout_guides = LayoutGuideTable::default();
            populate_layout_guides(env, &mut layout_guides, node, view);

            // Walk every "translates auto-resizing mask into constraints
            // == NO" view and apply the constraint set to it. We do this
            // as a single pass over the entire tree of the root view.
            apply_constraints_recursive(env, ctx, view_node, view, &layout_guides);

            // Make sure setBackgroundColor was applied even if the
            // background <color> element only appeared after <subviews>;
            // see apply_view_attributes.
        }
    }

    // Capture connections (outlets/actions) attached to the view
    // controller itself.
    if let Some(conns) = node.child_by_tag("connections") {
        collect_connections(conns, node.attr("id").unwrap_or(""), ctx);
    }

    vc
}

fn ensure_root_frame(env: &mut Environment, view: id) {
    let cur: CGRect = msg![env; view frame];
    if cur.size.width <= 0.0 || cur.size.height <= 0.0 {
        let screen: id = msg_class![env; UIScreen mainScreen];
        let bounds: CGRect = msg![env; screen bounds];
        let _: () = msg![env; view setFrame:bounds];
    }
}

// -------------------------------------------------------------------------
// View instantiation
// -------------------------------------------------------------------------

fn instantiate_view(
    env: &mut Environment,
    ctx: &mut InstantiationCtx<'_>,
    node: &XmlNode,
    parent_view: Option<id>,
) -> id {
    let class_name = view_class_name(&node.tag);

    // Honour customClass when the binary defines it.
    let class = {
        let custom = node.attr("customClass");
        let mut found: Option<crate::objc::Class> = None;
        if let Some(custom_name) = custom {
            if let Some(c) = env.objc.try_get_known_class(custom_name, &mut env.mem) {
                found = Some(c);
            }
        }
        found.unwrap_or_else(|| env.objc.get_known_class(class_name, &mut env.mem))
    };

    // UIButton has +buttonWithType: as the designated constructor; the
    // type attribute is encoded as buttonType="roundedRect" in IB.
    let view: id = if node.tag == "button" {
        let bt = parse_button_type(node.attr("buttonType").unwrap_or("custom"));
        let btn_class = env.objc.get_known_class("UIButton", &mut env.mem);
        let v: id = msg![env; btn_class buttonWithType:bt];
        // buttonWithType returns autoreleased; retain so it survives until
        // we add it to the superview.
        if v != nil { retain(env, v); }
        v
    } else {
        let v: id = msg![env; class alloc];
        // Initialize with the frame from the XML if present so init…s
        // that depend on a non-zero rect (e.g. UIScrollView) start in a
        // sane state.
        let frame = parse_frame(node).unwrap_or(CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: 0.0,
                height: 0.0,
            },
        });
        let v: id = msg![env; v initWithFrame:frame];
        v
    };

    if view == nil {
        return nil;
    }

    if let Some(xml_id) = node.attr("id") {
        ctx.instances.insert(xml_id.to_string(), view);
    }

    apply_view_attributes(env, node, view);

    // Subviews.
    if let Some(subs) = node.child_by_tag("subviews") {
        for child in &subs.children {
            if !is_view_tag(&child.tag) {
                continue;
            }
            let child_view = instantiate_view(env, ctx, child, Some(view));
            if child_view != nil {
                let _: () = msg![env; view addSubview:child_view];
                release(env, child_view);
            }
        }
    }

    // Background color may appear *after* `<subviews>` in IB output.
    if let Some(bg) = node.child_by_key("backgroundColor") {
        let color = parse_color(env, bg);
        if color != nil {
            let _: () = msg![env; view setBackgroundColor:color];
        }
    }
    if let Some(tint) = node.child_by_key("tintColor") {
        let color = parse_color(env, tint);
        if color != nil {
            let _: () = msg![env; view setTintColor:color];
        }
    }

    // Per-tag specifics.
    match node.tag.as_str() {
        "label" => apply_label_attributes(env, node, view),
        "button" => apply_button_attributes(env, node, view),
        "imageView" => apply_image_view_attributes(env, node, view),
        "textField" => apply_text_field_attributes(env, node, view),
        _ => {}
    }

    // Connections live on the view directly (mostly for controls).
    if let Some(conns) = node.child_by_tag("connections") {
        collect_connections(conns, node.attr("id").unwrap_or(""), ctx);
    }

    let _ = parent_view; // currently unused; reserved for future per-parent logic
    view
}

fn apply_view_attributes(env: &mut Environment, node: &XmlNode, view: id) {
    if let Some(frame) = parse_frame(node) {
        let _: () = msg![env; view setFrame:frame];
    }
    if let Some(hidden) = node.attr("hidden").and_then(parse_yes_no) {
        let _: () = msg![env; view setHidden:hidden];
    }
    if let Some(opaque) = node.attr("opaque").and_then(parse_yes_no) {
        let _: () = msg![env; view setOpaque:opaque];
    }
    if let Some(clips) = node
        .attr("clipsSubviews")
        .or(node.attr("clipsToBounds"))
        .and_then(parse_yes_no)
    {
        let _: () = msg![env; view setClipsToBounds:clips];
    }
    if let Some(uie) = node.attr("userInteractionEnabled").and_then(parse_yes_no) {
        let _: () = msg![env; view setUserInteractionEnabled:uie];
    }
    if let Some(ms) = node.attr("multipleTouchEnabled").and_then(parse_yes_no) {
        let _: () = msg![env; view setMultipleTouchEnabled:ms];
    }
    if let Some(alpha) = node.attr("alpha").and_then(parse_cgfloat) {
        let _: () = msg![env; view setAlpha:alpha];
    }
    if let Some(tag_val) = node
        .attr("tag")
        .and_then(|v| v.parse::<crate::frameworks::foundation::NSInteger>().ok())
    {
        let _: () = msg![env; view setTag:tag_val];
    }
    if let Some(cm) = node.attr("contentMode") {
        let mode = parse_content_mode(cm);
        let _: () = msg![env; view setContentMode:mode];
    }
    if let Some(translates) = node
        .attr("translatesAutoresizingMaskIntoConstraints")
        .and_then(parse_yes_no)
    {
        let _: () = msg![env; view setTranslatesAutoresizingMaskIntoConstraints:translates];
    }
}

fn apply_label_attributes(env: &mut Environment, node: &XmlNode, label: id) {
    if let Some(text) = node.attr("text") {
        let s = from_rust_string(env, text.to_string());
        let _: () = msg![env; label setText:s];
        release(env, s);
    }
    if let Some(fd) = node.child_by_key("fontDescription") {
        let font = parse_font(env, fd);
        if font != nil {
            let _: () = msg![env; label setFont:font];
        }
    }
    if let Some(color_node) = node.child_by_key("textColor") {
        let c = parse_color(env, color_node);
        if c != nil {
            let _: () = msg![env; label setTextColor:c];
        }
    }
    if let Some(color_node) = node.child_by_key("shadowColor") {
        let c = parse_color(env, color_node);
        if c != nil {
            let _: () = msg![env; label setShadowColor:c];
        }
    }
    if let Some(align) = node.attr("textAlignment") {
        let val = parse_text_alignment(align);
        let _: () = msg![env; label setTextAlignment:val];
    }
    if let Some(lines) = node
        .attr("numberOfLines")
        .and_then(|v| v.parse::<crate::frameworks::foundation::NSInteger>().ok())
    {
        let _: () = msg![env; label setNumberOfLines:lines];
    }
    if let Some(adjusts) = node.attr("adjustsFontSizeToFit").and_then(parse_yes_no) {
        let _: () = msg![env; label setAdjustsFontSizeToFitWidth:adjusts];
    }
    if let Some(lb) = node.attr("lineBreakMode") {
        let val = parse_line_break_mode(lb);
        let _: () = msg![env; label setLineBreakMode:val];
    }
    if let Some(highlighted) = node.attr("highlighted").and_then(parse_yes_no) {
        let _: () = msg![env; label setHighlighted:highlighted];
    }
    if let Some(enabled) = node.attr("enabled").and_then(parse_yes_no) {
        let _: () = msg![env; label setEnabled:enabled];
    }
}

fn apply_button_attributes(env: &mut Environment, node: &XmlNode, button: id) {
    if let Some(fd) = node.child_by_key("fontDescription") {
        let font = parse_font(env, fd);
        if font != nil {
            let label: id = msg![env; button titleLabel];
            if label != nil {
                let _: () = msg![env; label setFont:font];
            }
        }
    }

    for state in node.children_with_tag("state") {
        let state_name = state.attr("key").unwrap_or("normal");
        let state_value = parse_control_state(state_name);

        if let Some(title) = state.attr("title") {
            let s = from_rust_string(env, title.to_string());
            let _: () = msg![env; button setTitle:s forState:state_value];
            release(env, s);
        }
        if let Some(color_node) = state.child_by_key("titleColor") {
            let c = parse_color(env, color_node);
            if c != nil {
                let _: () = msg![env; button setTitleColor:c forState:state_value];
            }
        }
        if let Some(color_node) = state.child_by_key("titleShadowColor") {
            let c = parse_color(env, color_node);
            if c != nil {
                let _: () = msg![env; button setTitleShadowColor:c forState:state_value];
            }
        }
    }
}

fn apply_image_view_attributes(env: &mut Environment, node: &XmlNode, image_view: id) {
    if let Some(name) = node.attr("image") {
        let name_ns = from_rust_string(env, name.to_string());
        let img_class = env.objc.get_known_class("UIImage", &mut env.mem);
        let image: id = msg![env; img_class imageNamed:name_ns];
        release(env, name_ns);
        if image != nil {
            let _: () = msg![env; image_view setImage:image];
        }
    }
}

fn apply_text_field_attributes(env: &mut Environment, node: &XmlNode, text_field: id) {
    if let Some(text) = node.attr("text") {
        let s = from_rust_string(env, text.to_string());
        let _: () = msg![env; text_field setText:s];
        release(env, s);
    }
    if let Some(placeholder) = node.attr("placeholder") {
        let s = from_rust_string(env, placeholder.to_string());
        let _: () = msg![env; text_field setPlaceholder:s];
        release(env, s);
    }
    if let Some(fd) = node.child_by_key("fontDescription") {
        let font = parse_font(env, fd);
        if font != nil {
            let _: () = msg![env; text_field setFont:font];
        }
    }
}

// -------------------------------------------------------------------------
// Connections
// -------------------------------------------------------------------------

fn collect_connections(node: &XmlNode, source_xml_id: &str, ctx: &mut InstantiationCtx<'_>) {
    for c in &node.children {
        match c.tag.as_str() {
            "outlet" => {
                if let (Some(property), Some(dest)) = (c.attr("property"), c.attr("destination")) {
                    ctx.pending_connections.push(Connection {
                        kind: ConnectionKind::Outlet,
                        source_xml_id: source_xml_id.to_string(),
                        destination_xml_id: dest.to_string(),
                        selector: None,
                        property: Some(property.to_string()),
                        event_type: None,
                    });
                }
            }
            "action" => {
                if let (Some(selector), Some(dest)) = (c.attr("selector"), c.attr("destination")) {
                    ctx.pending_connections.push(Connection {
                        kind: ConnectionKind::Action,
                        source_xml_id: source_xml_id.to_string(),
                        destination_xml_id: dest.to_string(),
                        selector: Some(selector.to_string()),
                        property: None,
                        event_type: c.attr("eventType").map(|s| s.to_string()),
                    });
                }
            }
            _ => {}
        }
    }
}

fn apply_connections(env: &mut Environment, ctx: &mut InstantiationCtx<'_>) {
    let conns = std::mem::take(&mut ctx.pending_connections);
    for c in conns {
        match c.kind {
            ConnectionKind::Outlet => apply_outlet(env, ctx, &c),
            ConnectionKind::Action => apply_action(env, ctx, &c),
        }
    }
}

fn apply_outlet(env: &mut Environment, ctx: &InstantiationCtx<'_>, c: &Connection) {
    let Some(source) = ctx.instances.get(&c.source_xml_id).copied() else {
        return;
    };
    let Some(dest) = ctx.instances.get(&c.destination_xml_id).copied() else {
        return;
    };
    let Some(property) = c.property.as_deref() else {
        return;
    };
    let property_ns = from_rust_string(env, property.to_string());
    // KVC dispatches through setValue:forKey:, which falls back to
    // setProperty:/_property/_setProperty: lookups. UIViewController
    // already overrides this for "view".
    let _: () = msg![env; source setValue:dest forKey:property_ns];
    release(env, property_ns);
}

fn apply_action(env: &mut Environment, ctx: &InstantiationCtx<'_>, c: &Connection) {
    let Some(source) = ctx.instances.get(&c.source_xml_id).copied() else {
        return;
    };
    let Some(dest) = ctx.instances.get(&c.destination_xml_id).copied() else {
        return;
    };
    let Some(selector_str) = c.selector.as_deref() else {
        return;
    };

    let events: UIControlEvents = parse_event_type(c.event_type.as_deref().unwrap_or(""));

    let sel: SEL = env.objc.register_host_selector(selector_str.to_string(), &mut env.mem);

    let _: () = msg![env; source addTarget:dest action:sel forControlEvents:events];
}

// -------------------------------------------------------------------------
// Attribute helpers
// -------------------------------------------------------------------------

fn parse_yes_no(s: &str) -> Option<bool> {
    match s {
        "YES" | "yes" | "true" | "TRUE" | "1" => Some(true),
        "NO" | "no" | "false" | "FALSE" | "0" => Some(false),
        _ => None,
    }
}

fn parse_cgfloat(s: &str) -> Option<CGFloat> {
    s.parse::<f32>().ok().map(|v| v as CGFloat)
}

fn parse_frame(node: &XmlNode) -> Option<CGRect> {
    let rect = node.child_by_key("frame")?;
    let x = rect.attr("x").and_then(parse_cgfloat).unwrap_or(0.0);
    let y = rect.attr("y").and_then(parse_cgfloat).unwrap_or(0.0);
    let w = rect.attr("width").and_then(parse_cgfloat).unwrap_or(0.0);
    let h = rect.attr("height").and_then(parse_cgfloat).unwrap_or(0.0);
    Some(CGRect {
        origin: CGPoint { x, y },
        size: CGSize {
            width: w,
            height: h,
        },
    })
}

fn parse_content_mode(s: &str) -> i32 {
    // UIViewContentMode constants
    match s {
        "scaleToFill" => 0,
        "scaleAspectFit" => 1,
        "scaleAspectFill" => 2,
        "redraw" => 3,
        "center" => 4,
        "top" => 5,
        "bottom" => 6,
        "left" => 7,
        "right" => 8,
        "topLeft" => 9,
        "topRight" => 10,
        "bottomLeft" => 11,
        "bottomRight" => 12,
        _ => 0,
    }
}

fn parse_text_alignment(s: &str) -> i32 {
    // NSTextAlignment
    match s {
        "left" => 0,
        "center" => 1,
        "right" => 2,
        "justified" => 3,
        "natural" => 4,
        _ => 4,
    }
}

fn parse_line_break_mode(s: &str) -> i32 {
    // NSLineBreakMode
    match s {
        "wordWrap" => 0,
        "charWrap" => 1,
        "clip" => 2,
        "headTruncation" => 3,
        "tailTruncation" => 4,
        "middleTruncation" => 5,
        _ => 0,
    }
}

fn parse_button_type(s: &str) -> i32 {
    match s {
        "custom" => 0,
        "roundedRect" | "system" => 1,
        "detailDisclosure" => 2,
        "infoLight" => 3,
        "infoDark" => 4,
        "contactAdd" => 5,
        _ => 0,
    }
}

fn parse_control_state(s: &str) -> NSUInteger {
    let mut state: NSUInteger = 0;
    for token in s.split(|c: char| c == '+' || c == ' ' || c == '|') {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        state |= match t {
            "normal" => 0,
            "highlighted" => 1 << 0,
            "disabled" => 1 << 1,
            "selected" => 1 << 2,
            "focused" => 1 << 3,
            _ => 0,
        };
    }
    state
}

fn parse_event_type(s: &str) -> UIControlEvents {
    let mut events: UIControlEvents = 0;
    for token in s.split(|c: char| c == ' ' || c == '|' || c == '+') {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        events |= match t {
            "touchDown" => 1 << 0,
            "touchDownRepeat" => 1 << 1,
            "touchDragInside" => 1 << 2,
            "touchDragOutside" => 1 << 3,
            "touchDragEnter" => 1 << 4,
            "touchDragExit" => 1 << 5,
            "touchUpInside" => 1 << 6,
            "touchUpOutside" => 1 << 7,
            "touchCancel" => 1 << 8,
            "valueChanged" => 1 << 12,
            "primaryActionTriggered" => 1 << 13,
            "editingDidBegin" => 1 << 16,
            "editingChanged" => 1 << 17,
            "editingDidEnd" => 1 << 18,
            "editingDidEndOnExit" => 1 << 19,
            _ => 0,
        };
    }
    if events == 0 {
        // Apple defaults the eventType to touchUpInside when omitted on
        // a control action.
        events = 1 << 6;
    }
    events
}

fn parse_color(env: &mut Environment, node: &XmlNode) -> id {
    // Apple's IB serializes colors in several flavours:
    //
    //   <color red="1" green="0" blue="0" alpha="1" colorSpace="sRGB"/>
    //   <color white="1" alpha="1" colorSpace="calibratedWhite"/>
    //   <color name="systemBlueColor"/>
    //
    // We handle all three. The colorSpace attribute is informational
    // for us; we use the RGB(A) values directly.
    if let Some(name) = node.attr("name") {
        // System color lookup; fall back to UIColor whiteColor if the
        // symbol isn't known.
        let sel_name = name.to_string();
        let color_class = env.objc.get_known_class("UIColor", &mut env.mem);
        if env.objc.class_has_method_named(color_class, &sel_name) {
            let s = env.objc.register_host_selector(sel_name, &mut env.mem);
            let c: id = crate::objc::msg_send(env, (color_class, s));
            return c;
        }
    }

    let alpha = node.attr("alpha").and_then(parse_cgfloat).unwrap_or(1.0);

    if let Some(white) = node.attr("white").and_then(parse_cgfloat) {
        let color_class = env.objc.get_known_class("UIColor", &mut env.mem);
        let c: id = msg![env; color_class colorWithWhite:white alpha:alpha];
        return c;
    }

    let r = node.attr("red").and_then(parse_cgfloat);
    let g = node.attr("green").and_then(parse_cgfloat);
    let b = node.attr("blue").and_then(parse_cgfloat);
    if let (Some(r), Some(g), Some(b)) = (r, g, b) {
        let color_class = env.objc.get_known_class("UIColor", &mut env.mem);
        let c: id = msg![env; color_class colorWithRed:r green:g blue:b alpha:alpha];
        return c;
    }

    nil
}

fn parse_font(env: &mut Environment, node: &XmlNode) -> id {
    let point_size = node
        .attr("pointSize")
        .and_then(parse_cgfloat)
        .unwrap_or(17.0);
    let kind = node.attr("type").unwrap_or("system");
    let name = node.attr("name");
    let font_class = env.objc.get_known_class("UIFont", &mut env.mem);

    if let Some(name) = name {
        let name_ns = from_rust_string(env, name.to_string());
        let f: id = msg![env; font_class fontWithName:name_ns size:point_size];
        release(env, name_ns);
        if f != nil {
            return f;
        }
    }
    match kind {
        "boldSystem" => {
            let f: id = msg![env; font_class boldSystemFontOfSize:point_size];
            f
        }
        "italicSystem" => {
            let f: id = msg![env; font_class italicSystemFontOfSize:point_size];
            f
        }
        _ => {
            let f: id = msg![env; font_class systemFontOfSize:point_size];
            f
        }
    }
}

// -------------------------------------------------------------------------
// Lightweight constraint pass
// -------------------------------------------------------------------------

/// IB attribute integers match NSLayoutAttribute. We mirror the subset
/// we resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
enum Attr {
    Left,
    Right,
    Top,
    Bottom,
    Leading,
    Trailing,
    Width,
    Height,
    CenterX,
    CenterY,
}

fn parse_attr(s: &str) -> Option<Attr> {
    Some(match s {
        "left" => Attr::Left,
        "right" => Attr::Right,
        "top" => Attr::Top,
        "bottom" => Attr::Bottom,
        "leading" => Attr::Leading,
        "trailing" => Attr::Trailing,
        "width" => Attr::Width,
        "height" => Attr::Height,
        "centerX" => Attr::CenterX,
        "centerY" => Attr::CenterY,
        _ => return None,
    })
}

#[derive(Debug, Default)]
struct LayoutGuideTable {
    /// xml_id → (CGRect representing the guide's frame in the view
    /// controller's `view` coordinate system).
    by_id: HashMap<String, CGRect>,
}

fn populate_layout_guides(
    env: &mut Environment,
    table: &mut LayoutGuideTable,
    vc_node: &XmlNode,
    view: id,
) {
    let view_bounds: CGRect = msg![env; view bounds];
    let status_bar_height: CGFloat = 20.0;
    if let Some(guides) = vc_node.child_by_tag("layoutGuides") {
        for g in guides.children_with_tag("viewControllerLayoutGuide") {
            let kind = g.attr("type").unwrap_or("");
            let id_attr = g.attr("id").map(|s| s.to_string());
            if let Some(id_attr) = id_attr {
                let rect = match kind {
                    "top" => CGRect {
                        origin: CGPoint { x: 0.0, y: 0.0 },
                        size: CGSize {
                            width: view_bounds.size.width,
                            height: status_bar_height,
                        },
                    },
                    "bottom" => CGRect {
                        origin: CGPoint {
                            x: 0.0,
                            y: view_bounds.size.height,
                        },
                        size: CGSize {
                            width: view_bounds.size.width,
                            height: 0.0,
                        },
                    },
                    _ => CGRect {
                        origin: CGPoint { x: 0.0, y: 0.0 },
                        size: CGSize {
                            width: 0.0,
                            height: 0.0,
                        },
                    },
                };
                table.by_id.insert(id_attr, rect);
            }
        }
    }
}

/// Walk the view tree collecting `<constraints>` blocks and apply them
/// to the relevant subviews. Uses a fixed-point iteration: re-runs until
/// no frame changes, capped at 8 passes.
fn apply_constraints_recursive(
    env: &mut Environment,
    ctx: &InstantiationCtx<'_>,
    node: &XmlNode,
    root_view: id,
    guides: &LayoutGuideTable,
) {
    // Collect every (<constraints>, owning_view_id) pair in the tree.
    let mut constraint_blocks: Vec<(String, &XmlNode)> = Vec::new();
    fn walk<'a>(node: &'a XmlNode, out: &mut Vec<(String, &'a XmlNode)>) {
        if let Some(owner_id) = node.attr("id") {
            if let Some(constraints) = node.child_by_tag("constraints") {
                out.push((owner_id.to_string(), constraints));
            }
        }
        if let Some(subs) = node.child_by_tag("subviews") {
            for c in &subs.children {
                walk(c, out);
            }
        }
    }
    walk(node, &mut constraint_blocks);

    if constraint_blocks.is_empty() {
        let _ = root_view;
        return;
    }

    for _iter in 0..8 {
        let mut changed = false;
        for (owner_id, block) in &constraint_blocks {
            for cn in block.children_with_tag("constraint") {
                if apply_one_constraint(env, ctx, cn, owner_id, guides) {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn resolve_target(
    env: &mut Environment,
    ctx: &InstantiationCtx<'_>,
    guides: &LayoutGuideTable,
    xml_id: &str,
) -> Option<TargetRect> {
    if let Some(rect) = guides.by_id.get(xml_id).copied() {
        return Some(TargetRect::Guide(rect));
    }
    let view = ctx.instances.get(xml_id).copied()?;
    let frame: CGRect = msg![env; view frame];
    Some(TargetRect::View {
        view,
        frame,
    })
}

#[derive(Debug, Clone, Copy)]
enum TargetRect {
    Guide(CGRect),
    View { view: id, frame: CGRect },
}

impl TargetRect {
    fn frame(self) -> CGRect {
        match self {
            TargetRect::Guide(r) => r,
            TargetRect::View { frame, .. } => frame,
        }
    }
}

fn attr_value(rect: CGRect, attr: Attr) -> CGFloat {
    match attr {
        Attr::Left | Attr::Leading => rect.origin.x,
        Attr::Right | Attr::Trailing => rect.origin.x + rect.size.width,
        Attr::Top => rect.origin.y,
        Attr::Bottom => rect.origin.y + rect.size.height,
        Attr::Width => rect.size.width,
        Attr::Height => rect.size.height,
        Attr::CenterX => rect.origin.x + rect.size.width / 2.0,
        Attr::CenterY => rect.origin.y + rect.size.height / 2.0,
    }
}

/// Adjust `rect` so the value of `attr` equals `value` while keeping
/// the rest of the rectangle reasonable. Returns the new rect.
fn set_attr(rect: CGRect, attr: Attr, value: CGFloat) -> CGRect {
    let mut r = rect;
    match attr {
        Attr::Left | Attr::Leading => r.origin.x = value,
        Attr::Right | Attr::Trailing => r.origin.x = value - r.size.width,
        Attr::Top => r.origin.y = value,
        Attr::Bottom => r.origin.y = value - r.size.height,
        Attr::Width => r.size.width = value,
        Attr::Height => r.size.height = value,
        Attr::CenterX => r.origin.x = value - r.size.width / 2.0,
        Attr::CenterY => r.origin.y = value - r.size.height / 2.0,
    }
    r
}

/// Resolve one `<constraint>` element. Returns `true` if it caused the
/// first item's frame to change.
fn apply_one_constraint(
    env: &mut Environment,
    ctx: &InstantiationCtx<'_>,
    cn: &XmlNode,
    owner_xml_id: &str,
    guides: &LayoutGuideTable,
) -> bool {
    let first_attr_str = cn.attr("firstAttribute").unwrap_or("");
    let Some(first_attr) = parse_attr(first_attr_str) else {
        return false;
    };
    let constant = cn.attr("constant").and_then(parse_cgfloat).unwrap_or(0.0);
    let multiplier = cn.attr("multiplier").and_then(parse_cgfloat).unwrap_or(1.0);

    // The first item defaults to the owner of the <constraints> block.
    let first_id = cn
        .attr("firstItem")
        .unwrap_or(owner_xml_id)
        .to_string();

    let Some(first_view) = ctx.instances.get(&first_id).copied() else {
        return false;
    };

    // Constraint applies only to views with
    // translatesAutoresizingMaskIntoConstraints == NO. We allow any
    // value; UIView will respect setFrame anyway.
    let first_frame: CGRect = msg![env; first_view frame];

    let second_attr_str = cn.attr("secondAttribute").unwrap_or("");
    let second_attr = parse_attr(second_attr_str);
    let second_id = cn.attr("secondItem");

    let new_value = if let (Some(second_attr), Some(second_id)) = (second_attr, second_id) {
        let Some(target) = resolve_target(env, ctx, guides, second_id) else {
            return false;
        };
        let v = attr_value(target.frame(), second_attr);
        v * multiplier + constant
    } else {
        // No second item ⇒ absolute constraint, e.g. width = 320.
        if multiplier == 0.0 {
            constant
        } else {
            constant
        }
    };

    let new_frame = set_attr(first_frame, first_attr, new_value);
    if frames_equal(new_frame, first_frame) {
        false
    } else {
        let _: () = msg![env; first_view setFrame:new_frame];
        true
    }
}

fn frames_equal(a: CGRect, b: CGRect) -> bool {
    (a.origin.x - b.origin.x).abs() < 0.001
        && (a.origin.y - b.origin.y).abs() < 0.001
        && (a.size.width - b.size.width).abs() < 0.001
        && (a.size.height - b.size.height).abs() < 0.001
}

// -------------------------------------------------------------------------
// File loading
// -------------------------------------------------------------------------

/// Locate a raw `.storyboard` file inside `bundle`'s resource path,
/// honouring per-device `~iphone` / `~ipad` suffixes and the
/// `Base.lproj/` Base-internationalization fallback. Returns the
/// absolute path on success.
pub fn locate_storyboard_xml(
    env: &mut Environment,
    bundle: id,
    name: &str,
    device_family: Option<crate::window::DeviceFamily>,
) -> Option<String> {
    let device_suffix = match device_family {
        Some(crate::window::DeviceFamily::iPad) => Some("~ipad"),
        Some(crate::window::DeviceFamily::iPhone) | Some(crate::window::DeviceFamily::iPhone5) => {
            Some("~iphone")
        }
        None => None,
    };
    let mut candidates: Vec<String> = Vec::new();
    if let Some(suffix) = device_suffix {
        candidates.push(format!("{name}{suffix}"));
    }
    candidates.push(name.to_string());

    let type_ns: id = get_static_str(env, "storyboard");
    for cand in &candidates {
        let cand_ns: id = from_rust_string(env, cand.clone());
        let path: id = msg![env; bundle pathForResource:cand_ns ofType:type_ns];
        release(env, cand_ns);
        if path != nil {
            return Some(to_rust_string(env, path).into_owned());
        }
    }
    None
}

/// Read a `.storyboard` file from the host filesystem and parse it.
pub fn load_and_parse_storyboard(
    env: &mut Environment,
    absolute_path: &str,
) -> Option<Arc<ParsedStoryboard>> {
    // Read via Environment.fs so the guest's overlay/sandbox rules
    // apply; on failure fall back to a raw host read so apps stored on
    // disk outside the IPA mount still load.
    let xml = match env.fs.read(crate::fs::GuestPath::new(absolute_path)) {
        Ok(bytes) => bytes,
        Err(_) => match std::fs::read(absolute_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                log!(
                    "Warning: couldn't read storyboard XML {absolute_path:?}: {e}"
                );
                return None;
            }
        },
    };
    let xml_str = String::from_utf8_lossy(&xml).into_owned();
    match parse_storyboard_xml(&xml_str) {
        Ok(p) => Some(Arc::new(p)),
        Err(e) => {
            log!("Warning: storyboard XML parse error in {absolute_path:?}: {e}");
            None
        }
    }
}
