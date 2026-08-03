use std::collections::HashMap;
use std::io::{Cursor, Seek, SeekFrom};

use mach_object::{LoadCommand, MachCommand, OFile, Symbol, SymbolIter, ThreadState};

use crate::mem64::{Guest64Addr, Mem64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture { Arm64 }

#[derive(Debug)]
pub struct MachO64 {
    pub architecture: Architecture,
    pub name: String,
    pub dynamic_libraries: Vec<String>,
    pub exported_symbols: HashMap<String, Guest64Addr>,
    pub entry_point_pc: Option<Guest64Addr>,
    pub text_base: Guest64Addr,
    pub last_segment_end: Guest64Addr,
    pub memory: Mem64,
}

impl MachO64 {
    pub fn load_from_bytes(bytes: &[u8], name: impl Into<String>, slide: u64) -> Result<Self, String> {
        let name = name.into();
        let mut cursor = Cursor::new(bytes);
        let file = OFile::parse(&mut cursor).map_err(|e| format!("could not parse ARM64 Mach-O: {e}"))?;
        let file = match file {
            OFile::FatFile { files, .. } => files.into_iter().find(|(arch, _)| arch.cputype == mach_object::CPU_TYPE_ARM64).map(|(_, file)| file).ok_or("fat binary has no ARM64 slice")?,
            file => file,
        };
        let (header, commands) = match file { OFile::MachFile { header, commands } => (header, commands), _ => return Err("ARM64 input is not an executable Mach-O".into()) };
        if header.cputype != mach_object::CPU_TYPE_ARM64 || !header.is_64bit() { return Err("Mach-O is not an ARM64 64-bit image".into()); }
        if header.is_bigend() { return Err("ARM64 Mach-O is big-endian".into()); }

        let mut memory = Mem64::new();
        let mut dynamic_libraries = Vec::new();
        let mut exported_symbols = HashMap::new();
        let mut text_base = None;
        let mut last_segment_end = 0;
        let mut entry_point_pc = None;
        let mut symtab = None;
        let mut sections = Vec::new();

        for MachCommand(command, _) in commands {
            match command {
                LoadCommand::Segment64 { segname, vmaddr, vmsize, fileoff, filesize, sections: segment_sections, .. } => {
                    let base = vmaddr.checked_add(slide).ok_or("segment address overflows")?;
                    last_segment_end = last_segment_end.max(base.checked_add(vmsize as u64).ok_or("segment end overflows")?);
                    if segname == "__PAGEZERO" { continue; }
                    if segname == "__TEXT" { text_base = Some(base); }
                    if vmsize == 0 { continue; }
                    memory.map_zeroed(base, vmsize as u64)?;
                    if filesize != 0 {
                        let start = usize::try_from(fileoff).map_err(|_| "segment file offset is too large")?;
                        let length = usize::try_from(filesize).map_err(|_| "segment file size is too large")?;
                        let end = start.checked_add(length).ok_or("segment file range overflows")?;
                        if end > bytes.len() { return Err(format!("segment {segname} extends past the Mach-O file")); }
                        memory.write_bytes(base, &bytes[start..end])?;
                    }
                    sections.extend(segment_sections);
                }
                LoadCommand::SymTab { symoff, nsyms, stroff, strsize } => symtab = Some((symoff, nsyms, stroff, strsize)),
                LoadCommand::LoadDyLib(lib) => dynamic_libraries.push(lib.name.to_string()),
                LoadCommand::EncryptionInfo64 { id, .. } if id != 0 => return Err("ARM64 executable is encrypted".into()),
                LoadCommand::EntryPoint { entryoff, .. } => entry_point_pc = Some(text_base.ok_or("LC_MAIN appeared before __TEXT")?.checked_add(entryoff).ok_or("entry point overflows")?),
                LoadCommand::UnixThread { state: ThreadState::Arm64 { __pc, .. }, .. } => entry_point_pc = Some(__pc.checked_add(slide).ok_or("entry point overflows")?),
                _ => {}
            }
        }

        if let Some((symoff, nsyms, stroff, strsize)) = symtab {
            let mut symbols_cursor = cursor.clone();
            symbols_cursor.seek(SeekFrom::Start(symoff as u64)).map_err(|_| "invalid symbol table offset")?;
            for symbol in SymbolIter::new(&mut symbols_cursor, sections, nsyms, stroff, strsize, false, true) {
                if let Symbol::Defined { name: Some(symbol_name), entry, .. } = symbol { exported_symbols.insert(symbol_name.to_string(), entry as u64 + slide); }
            }
        }

        Ok(Self { architecture: Architecture::Arm64, name, dynamic_libraries, exported_symbols, entry_point_pc, text_base: text_base.unwrap_or(0), last_segment_end, memory })
    }
}
