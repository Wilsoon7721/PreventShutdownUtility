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

pub fn parse_smbios_chassis(raw: &[u8]) -> Option<u8> {
    let len = u32::from_le_bytes(raw.get(4..8)?.try_into().ok()?) as usize;
    let table = raw.get(8..8 + len)?;
    let mut i = 0;
    while i + 4 <= table.len() {
        let ty = table[i];
        let formatted = table[i + 1] as usize;
        if formatted < 4 || i + formatted > table.len() {
            return None;
        }
        if ty == 3 && formatted > 5 {
            return Some(table[i + 5] & 0x7F);
        }
        if ty == 127 {
            return None;
        }
        let mut j = i + formatted;
        while j + 1 < table.len() && !(table[j] == 0 && table[j + 1] == 0) {
            j += 1;
        }
        i = j + 2;
    }
    None
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
}
