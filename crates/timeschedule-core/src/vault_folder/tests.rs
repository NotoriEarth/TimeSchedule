use super::*;

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp() -> Temp {
    let root = std::env::temp_dir().join(format!("vault-folder-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("vault")).unwrap();
    fs::create_dir_all(root.join("outside")).unwrap();
    Temp(root)
}

impl Temp {
    fn vault(&self) -> PathBuf {
        self.0.join("vault")
    }
    fn outside(&self) -> PathBuf {
        self.0.join("outside")
    }
    fn outside_is_empty(&self) -> bool {
        fs::read_dir(self.outside()).unwrap().next().is_none()
    }
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
}

/// Makes the empty folder `dir` a junction to `target` in place, as a program holding
/// only the right to write its attributes can while the folder is open elsewhere.
#[cfg(windows)]
fn junction_in_place(dir: &Path, target: &Path) {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
    let file = fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(dir)
        .unwrap();
    let wide = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let substitute = wide(&format!(r"\??\{}", target.display()));
    let print = wide(&target.display().to_string());
    let mut names = Vec::new();
    names.extend(&substitute);
    names.push(0);
    names.extend(&print);
    names.push(0);
    let names: Vec<u8> = names.iter().flat_map(|c| c.to_le_bytes()).collect();
    let mut buffer = Vec::new();
    buffer.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend(((8 + names.len()) as u16).to_le_bytes());
    buffer.extend(0_u16.to_le_bytes());
    buffer.extend(0_u16.to_le_bytes());
    buffer.extend(((substitute.len() * 2) as u16).to_le_bytes());
    buffer.extend((((substitute.len() + 1) * 2) as u16).to_le_bytes());
    buffer.extend(((print.len() * 2) as u16).to_le_bytes());
    buffer.extend(&names);
    let mut returned = 0;
    // SAFETY: the buffer outlives the call and its length is given.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle() as _,
            FSCTL_SET_REPARSE_POINT,
            buffer.as_ptr().cast(),
            buffer.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    assert!(ok != 0, "{}", io::Error::last_os_error());
}

#[test]
fn files_are_made_read_replaced_and_listed_in_a_held_folder() {
    let t = temp();
    let dir = Folder::made(&t.vault(), &["daily", "a"]).unwrap();
    dir.publish("x.json", b"one").unwrap();
    dir.publish("x.json", b"two").unwrap();
    assert_eq!(dir.read("x.json").unwrap(), b"two");
    io::Write::write_all(&mut dir.open("log", Open::Append).unwrap(), b"1\n").unwrap();
    io::Write::write_all(&mut dir.open("log", Open::Append).unwrap(), b"2\n").unwrap();
    assert_eq!(dir.read("log").unwrap(), b"1\n2\n");
    let mut names = dir.names().unwrap();
    names.sort();
    assert_eq!(names, ["log", "x.json"], "no temporary file is left");
    assert_eq!(fs::read(t.vault().join("daily/a/x.json")).unwrap(), b"two");
    assert!(dir.read("missing").unwrap_err().kind() == io::ErrorKind::NotFound);
    assert!(Folder::found(&t.vault(), &["daily", "b"])
        .unwrap()
        .is_none());
    assert!(dir.find("..").is_err() && dir.open("a/b", Open::Read).is_err());
}

#[cfg(windows)]
#[test]
fn a_folder_or_file_that_is_a_junction_is_refused() {
    let t = temp();
    fs::write(t.outside().join("x.json"), b"outside").unwrap();
    junction(&t.vault().join("daily"), &t.outside());
    let error = Folder::made(&t.vault(), &["daily"]).err().unwrap();
    assert!(is_linked(&error), "{error}");
    let error = Folder::found(&t.vault(), &["daily"]).err().unwrap();
    assert!(is_linked(&error), "{error}");
    fs::remove_dir(t.vault().join("daily")).unwrap();
    let dir = Folder::made(&t.vault(), &["daily"]).unwrap();
    junction(&t.vault().join("daily").join("x.json"), &t.outside());
    for how in [Open::Read, Open::Lock, Open::Append] {
        let error = dir.open("x.json", how).unwrap_err();
        assert!(is_linked(&error), "{error}");
    }
    assert_eq!(fs::read(t.outside().join("x.json")).unwrap(), b"outside");
}

#[cfg(windows)]
#[test]
fn a_held_folder_and_the_folders_above_it_cannot_be_moved() {
    let t = temp();
    let dir = Folder::made(&t.vault(), &["project", "a", "id"]).unwrap();
    for at in ["project/a/id", "project/a", "project"] {
        let from = t.vault().join(at);
        assert!(
            fs::rename(&from, t.outside().join("moved")).is_err(),
            "{at} was moved"
        );
    }
    assert!(fs::rename(t.vault(), t.0.join("moved")).is_err());
    dir.publish("record.json", b"kept").unwrap();
    assert_eq!(
        fs::read(t.vault().join("project/a/id/record.json")).unwrap(),
        b"kept"
    );
    assert!(t.outside_is_empty());
    drop(dir);
    fs::rename(t.vault().join("project"), t.vault().join("moved")).unwrap();
}

#[cfg(windows)]
#[test]
fn an_empty_held_folder_made_a_junction_in_place_gets_nothing_made_or_read_through_it() {
    let t = temp();
    fs::write(t.outside().join("record.json"), b"outside").unwrap();
    let dir = Folder::made(&t.vault(), &["project", "a", "id"]).unwrap();
    junction_in_place(&t.vault().join("project/a/id"), &t.outside());
    assert!(fs::symlink_metadata(t.vault().join("project/a/id"))
        .unwrap()
        .file_type()
        .is_symlink());
    let error = dir.read("record.json").unwrap_err();
    assert!(is_linked(&error), "{error}");
    let error = dir.open(".record.lock", Open::Lock).unwrap_err();
    assert!(is_linked(&error), "{error}");
    let error = dir.open("usage.jsonl", Open::Append).unwrap_err();
    assert!(is_linked(&error), "{error}");
    let error = dir.publish("record.json", b"new").unwrap_err();
    assert!(is_linked(&error), "{error}");
    let error = dir.make("sub").err().unwrap();
    assert!(is_linked(&error), "{error}");
    let names: Vec<_> = fs::read_dir(t.outside()).unwrap().collect();
    assert_eq!(names.len(), 1, "only the file that was there");
    assert_eq!(
        fs::read(t.outside().join("record.json")).unwrap(),
        b"outside"
    );
}

/// Another run may be reading the latest record (the diary shows it) when a new one is
/// put in place. A reader that shares delete access, as every read here and
/// `std::fs` do, does not stop the new record.
#[test]
fn a_file_being_read_is_replaced_and_the_reader_keeps_what_it_opened() {
    let t = temp();
    let dir = Folder::made(&t.vault(), &["project", "a", "id"]).unwrap();
    dir.publish("record.json", b"old").unwrap();
    let mut held = dir.open("record.json", Open::Read).unwrap();
    let mut by_std = fs::File::open(t.vault().join("project/a/id/record.json")).unwrap();
    dir.publish("record.json", b"new").unwrap();
    assert_eq!(dir.read("record.json").unwrap(), b"new");
    for reader in [&mut held, &mut by_std] {
        let mut bytes = Vec::new();
        io::Read::read_to_end(reader, &mut bytes).unwrap();
        assert_eq!(bytes, b"old");
    }
}
