#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MachineInfo {
    pub chassis: Option<u8>,
    pub mobile_platform_role: Option<bool>,
    pub has_battery: bool,
}

const PORTABLE: &[u8] = &[8, 9, 10, 11, 14, 30, 31, 32];
const OTHER: u8 = 1;
const UNKNOWN: u8 = 2;

pub fn chassis_name(t: u8) -> &'static str {
    match t {
        1 => "Other",
        2 => "Unknown",
        3 => "Desktop",
        4 => "Low Profile Desktop",
        5 => "Pizza Box",
        6 => "Mini Tower",
        7 => "Tower",
        8 => "Portable",
        9 => "Laptop",
        10 => "Notebook",
        11 => "Hand Held",
        12 => "Docking Station",
        13 => "All in One",
        14 => "Sub Notebook",
        15 => "Space-saving",
        16 => "Lunch Box",
        17 => "Main Server Chassis",
        23 => "Rack Mount Chassis",
        24 => "Sealed-case PC",
        30 => "Tablet",
        31 => "Convertible",
        32 => "Detachable",
        33 => "IoT Gateway",
        34 => "Embedded PC",
        35 => "Mini PC",
        36 => "Stick PC",
        _ => "an unrecognised type",
    }
}

pub fn laptop_check(m: &MachineInfo) -> Result<(), String> {
    if !m.has_battery {
        return Err("Windows reports no battery in this computer.".into());
    }
    match m.chassis {
        Some(t) if PORTABLE.contains(&t) => Ok(()),
        Some(t) if t != OTHER && t != UNKNOWN => Err(format!(
            "This computer's firmware says it is a \"{}\" (chassis type {t}), not a laptop. \
             Some desktops and all-in-ones report a battery that never changes from 100%, \
             so the battery alone isn't trusted.",
            chassis_name(t)
        )),
        _ => match m.mobile_platform_role {
            Some(true) => Ok(()),
            _ => Err(
                "This computer's firmware doesn't identify it as a laptop, and Windows \
                 doesn't treat it as a mobile device."
                    .into(),
            ),
        },
    }
}

fn find_structure(raw: &[u8], want: u8) -> Option<(&[u8], &[u8])> {
    let len = u32::from_le_bytes(raw.get(4..8)?.try_into().ok()?) as usize;
    let table = raw.get(8..8 + len)?;
    let mut i = 0;
    while i + 4 <= table.len() {
        let ty = table[i];
        let formatted = table[i + 1] as usize;
        if formatted < 4 || i + formatted > table.len() {
            return None;
        }

        let mut j = i + formatted;
        while j + 1 < table.len() && !(table[j] == 0 && table[j + 1] == 0) {
            j += 1;
        }
        if ty == want {
            return Some((&table[i..i + formatted], table.get(i + formatted..j)?));
        }
        if ty == 127 {
            return None;
        }
        i = j + 2;
    }
    None
}

fn smbios_string(strings: &[u8], index: u8) -> Option<String> {
    if index == 0 {
        return None;
    }
    let raw = strings.split(|&b| b == 0).nth(index as usize - 1)?;
    let s = String::from_utf8_lossy(raw).trim().to_string();
    const PLACEHOLDERS: &[&str] = &[
        "to be filled by o.e.m.",
        "default string",
        "none",
        "not specified",
        "not available",
        "system serial number",
        "base board serial number",
        "o.e.m.",
        "oem",
        "x",
    ];
    let lower = s.to_ascii_lowercase();
    let blank = s.is_empty()
        || PLACEHOLDERS.contains(&lower.as_str())
        || s.chars()
            .all(|c| c == '0' || c == '.' || c == ' ' || c == '-');
    (!blank).then_some(s)
}

pub fn parse_smbios_chassis(raw: &[u8]) -> Option<u8> {
    let (formatted, _) = find_structure(raw, 3)?;
    Some(formatted.get(5)? & 0x7F)
}

/// SMBIOS type 1 system UUID: 16 bytes at offset 8
pub fn parse_smbios_uuid(raw: &[u8]) -> Option<[u8; 16]> {
    let (formatted, _) = find_structure(raw, 1)?;
    let uuid: [u8; 16] = formatted.get(8..24)?.try_into().ok()?;
    let blank = uuid.iter().all(|&b| b == 0) || uuid.iter().all(|&b| b == 0xFF);
    (!blank).then_some(uuid)
}

/// SMBIOS type 2 baseboard serial, fallback when the UUID is unusable
pub fn parse_smbios_baseboard_serial(raw: &[u8]) -> Option<String> {
    let (formatted, strings) = find_structure(raw, 2)?;
    smbios_string(strings, *formatted.get(7)?)
}

/// FNV-1a
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Which PC this is and what groups of PCs it belongs to
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Hashed per-machine key
    pub key: String,
    /// Where the key came from: "uuid", "board" or "name"
    pub source: &'static str,
    /// Computer name, upper-cased
    pub host: String,
    /// Name prefixes, shortest first
    pub prefixes: Vec<String>,
}

impl Identity {
    /// The narrowest group this PC belongs to - its room rather than its site.
    pub fn group(&self) -> Option<&str> {
        self.prefixes.last().map(String::as_str)
    }
}

pub fn identify(smbios: Option<&[u8]>, hostname: &str) -> Identity {
    let host = hostname.trim().to_ascii_uppercase();
    let by_name = || ("name", format!("name:{host}").into_bytes());
    let (source, seed) = match smbios {
        None => by_name(),
        Some(raw) => match parse_smbios_uuid(raw) {
            Some(uuid) => ("uuid", {
                let mut seed = b"uuid:".to_vec();
                seed.extend_from_slice(&uuid);
                seed
            }),
            None => match parse_smbios_baseboard_serial(raw) {
                Some(serial) => ("board", format!("board:{serial}").into_bytes()),
                None => by_name(),
            },
        },
    };
    Identity {
        key: format!("{:016x}", fnv64(&seed)),
        source,
        prefixes: host_prefixes(&host),
        host,
    }
}

/// Every prefix of the computer name that ends at a separator, shortest first.
/// SIT-L424-PC505 yields SIT- and SIT-L424-, so one observed shutdown can warn
/// for every PC in that room, and a hand-written SIT- line covers the site.
pub fn host_prefixes(host: &str) -> Vec<String> {
    let host = host.trim().to_ascii_uppercase();
    host.char_indices()
        .filter(|&(i, c)| matches!(c, '-' | '_' | '.') && i + c.len_utf8() < host.len())
        .map(|(i, c)| host[..i + c.len_utf8()].to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smbios(structs: &[&[u8]]) -> Vec<u8> {
        let table: Vec<u8> = structs.concat();
        let mut raw = vec![0, 3, 4, 0];
        raw.extend_from_slice(&(table.len() as u32).to_le_bytes());
        raw.extend(table);
        raw
    }

    const BIOS: &[u8] = &[0, 6, 0, 0, 1, 2, b'D', b'e', b'l', b'l', 0, b'1', 0, 0];
    const END: &[u8] = &[127, 4, 0xFF, 0xFF, 0, 0];

    fn enclosure(chassis_byte: u8) -> Vec<u8> {
        vec![3, 9, 1, 0, 1, chassis_byte, 0, 0, 0, b'X', 0, 0]
    }

    #[test]
    fn finds_chassis_after_other_structures() {
        let e = enclosure(9);
        assert_eq!(parse_smbios_chassis(&smbios(&[BIOS, &e, END])), Some(9));
    }

    #[test]
    fn chassis_lock_bit_is_masked() {
        let e = enclosure(0x80 | 13);
        assert_eq!(parse_smbios_chassis(&smbios(&[&e, END])), Some(13));
    }

    #[test]
    fn enclosure_without_strings() {
        let e = [3, 9, 1, 0, 0, 10, 0, 0, 0, 0, 0];
        assert_eq!(parse_smbios_chassis(&smbios(&[BIOS, &e, END])), Some(10));
    }

    #[test]
    fn missing_or_garbage_table() {
        assert_eq!(parse_smbios_chassis(&smbios(&[BIOS, END])), None);
        assert_eq!(parse_smbios_chassis(&[]), None);
        assert_eq!(parse_smbios_chassis(&smbios(&[&[3, 2, 0, 0]])), None);
    }

    fn m(chassis: Option<u8>, role: Option<bool>, battery: bool) -> MachineInfo {
        MachineInfo {
            chassis,
            mobile_platform_role: role,
            has_battery: battery,
        }
    }

    #[test]
    fn laptops_pass() {
        assert!(laptop_check(&m(Some(10), Some(true), true)).is_ok());
        assert!(laptop_check(&m(Some(31), Some(false), true)).is_ok());
        assert!(laptop_check(&m(Some(2), Some(true), true)).is_ok());
        assert!(laptop_check(&m(None, Some(true), true)).is_ok());
    }

    #[test]
    fn all_in_one_with_stuck_battery_is_rejected() {
        let err = laptop_check(&m(Some(13), Some(true), true)).unwrap_err();
        assert!(err.contains("All in One"), "{err}");
    }

    #[test]
    fn desktops_and_unknowns_are_rejected() {
        assert!(laptop_check(&m(Some(3), Some(false), true)).is_err());
        assert!(laptop_check(&m(Some(35), None, true)).is_err());
        assert!(laptop_check(&m(Some(1), Some(false), true)).is_err());
        assert!(laptop_check(&m(None, None, true)).is_err());
        assert!(laptop_check(&m(Some(9), Some(true), false)).is_err());
    }

    fn system(uuid: [u8; 16]) -> Vec<u8> {
        let mut v = vec![1, 27, 2, 0, 1, 2, 3, 4];
        v.extend_from_slice(&uuid);
        v.extend_from_slice(&[5, 6, 7]);
        v.extend_from_slice(b"Dell\0Latitude\0v1\0ABC123\0\0");
        v
    }

    fn board(serial: &[u8]) -> Vec<u8> {
        let mut v = vec![2, 15, 3, 0, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0];
        v.extend_from_slice(b"Dell\0ABCD\0A00\0");
        v.extend_from_slice(serial);
        v.extend_from_slice(b"\0\0");
        v
    }

    #[test]
    fn reads_system_uuid() {
        let uuid = [
            0x4C, 0x4C, 0x45, 0x44, 0x00, 0x37, 0x10, 0x59, 0x80, 0x43, 0xC3, 0xC0, 0x4F, 0x4B,
            0x33, 0x32,
        ];
        let sys = system(uuid);
        let e = enclosure(3);
        assert_eq!(
            parse_smbios_uuid(&smbios(&[BIOS, &sys, &e, END])),
            Some(uuid)
        );
        // The chassis parser still works with other structures around it.
        assert_eq!(
            parse_smbios_chassis(&smbios(&[BIOS, &sys, &e, END])),
            Some(3)
        );
    }

    #[test]
    fn blank_uuids_are_rejected() {
        for blank in [[0u8; 16], [0xFFu8; 16]] {
            let sys = system(blank);
            assert_eq!(parse_smbios_uuid(&smbios(&[&sys, END])), None);
        }
        assert_eq!(parse_smbios_uuid(&smbios(&[BIOS, END])), None);
    }

    #[test]
    fn reads_baseboard_serial_and_skips_placeholders() {
        let good = board(b"/8XK7P3/CN12345/");
        assert_eq!(
            parse_smbios_baseboard_serial(&smbios(&[&good, END])).as_deref(),
            Some("/8XK7P3/CN12345/")
        );
        for junk in [
            &b"Default string"[..],
            b"To be filled by O.E.M.",
            b"None",
            b"00000000",
            b"   ",
        ] {
            let b = board(junk);
            assert_eq!(
                parse_smbios_baseboard_serial(&smbios(&[&b, END])),
                None,
                "{}",
                String::from_utf8_lossy(junk)
            );
        }
    }

    #[test]
    fn prefixes_run_shortest_first_and_exclude_the_whole_name() {
        assert_eq!(
            host_prefixes("SIT-L424-PC505"),
            vec!["SIT-".to_string(), "SIT-L424-".to_string()]
        );
        assert_eq!(
            host_prefixes("sit-l424-pc505"),
            host_prefixes("SIT-L424-PC505")
        );
        assert_eq!(host_prefixes("LAPTOP"), Vec::<String>::new());
        // A trailing separator would make the prefix the entire name.
        assert_eq!(host_prefixes("SIT-"), Vec::<String>::new());
        assert_eq!(host_prefixes("A_B.C-D"), vec!["A_", "A_B.", "A_B.C-"]);
    }

    #[test]
    fn identity_prefers_firmware_then_board_then_name() {
        let uuid = [7u8; 16];
        let full = smbios(&[&system(uuid), &board(b"SER1"), END]);
        let id = identify(Some(&full), "sit-l424-pc505");
        assert_eq!(id.source, "uuid");
        assert_eq!(id.host, "SIT-L424-PC505");
        assert_eq!(id.group(), Some("SIT-L424-"));

        let no_uuid = smbios(&[&system([0; 16]), &board(b"SER1"), END]);
        assert_eq!(identify(Some(&no_uuid), "PC1").source, "board");

        let nothing = smbios(&[&system([0; 16]), &board(b"Default string"), END]);
        assert_eq!(identify(Some(&nothing), "PC1").source, "name");
        assert_eq!(identify(None, "PC1").source, "name");
    }

    #[test]
    fn identity_keys_are_stable_and_distinct() {
        let a = smbios(&[&system([1u8; 16]), END]);
        let b = smbios(&[&system([2u8; 16]), END]);
        // Same firmware, different name: still the same machine.
        assert_eq!(
            identify(Some(&a), "OLD-NAME").key,
            identify(Some(&a), "NEW-NAME").key
        );
        assert_ne!(identify(Some(&a), "X").key, identify(Some(&b), "X").key);
        // Name-derived keys are case-insensitive.
        assert_eq!(identify(None, "pc1").key, identify(None, "PC1").key);
        assert_eq!(identify(Some(&a), "X").key.len(), 16);
    }
}
