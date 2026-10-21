//! CivilOps PDF: `civilops-pdf.exe` carries exactly one application manifest, ours (Per-Monitor V2
//! DPI awareness, `src/windows_manifest.rs`). MinGW's GCC links a default manifest of its own, and
//! the linker kept both or only MinGW's until build.rs left it out.
#![cfg(windows)]

fn u16_at(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

/// The application manifests (resource type 24) a PE file carries, as text.
fn manifests(pe: &[u8]) -> Vec<String> {
    let header = u32_at(pe, 0x3C);
    assert_eq!(&pe[header..header + 4], b"PE\0\0");
    let sections = u16_at(pe, header + 6);
    let optional = header + 24;
    let directories = optional + if u16_at(pe, optional) == 0x20B { 112 } else { 96 };
    let table = optional + u16_at(pe, header + 20);
    // The file offset of a relative virtual address.
    let offset = |rva: usize| {
        (0..sections)
            .map(|i| table + i * 40)
            .find_map(|s| {
                let (size, start, raw) = (u32_at(pe, s + 8).max(u32_at(pe, s + 16)), u32_at(pe, s + 12), u32_at(pe, s + 20));
                (start..start + size).contains(&rva).then(|| raw + rva - start)
            })
            .unwrap()
    };
    let base = offset(u32_at(pe, directories + 2 * 8));
    // A resource directory's entries: (name or id, offset of what it points to).
    let entries = |dir: usize| {
        (0..u16_at(pe, dir + 12) + u16_at(pe, dir + 14)).map(move |k| (u32_at(pe, dir + 16 + 8 * k), u32_at(pe, dir + 20 + 8 * k) & 0x7FFF_FFFF))
    };
    let mut found = Vec::new();
    for (_, ids) in entries(base).filter(|(kind, _)| *kind == 24) {
        for (_, languages) in entries(base + ids) {
            for (_, leaf) in entries(base + languages) {
                let (rva, size) = (u32_at(pe, base + leaf), u32_at(pe, base + leaf + 4));
                found.push(String::from_utf8_lossy(&pe[offset(rva)..offset(rva) + size]).into_owned());
            }
        }
    }
    found
}

#[test]
fn the_program_carries_only_our_manifest() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_civilops-pdf")).unwrap();
    let found = manifests(&exe);
    assert_eq!(found.len(), 1, "application manifests in civilops-pdf.exe: {found:#?}");
    assert!(found[0].contains("PerMonitorV2"), "{}", found[0]);
}
