//! Folders of the document store, opened from the store's own folder one plain folder
//! at a time and held open while they are used.
//!
//! A daily or project record run waits minutes for the model, and a folder on the way
//! to its files (`daily/<folder>`, `project/<folder>/<project ID>`) can be made a link
//! meanwhile. Checking the path again before each write leaves a gap between the check
//! and the write. So on Windows a folder here is an open handle, and nothing is done by
//! path once it is open:
//!
//! - A sub-folder or file is opened by its one name relative to the folder's handle,
//!   without following a link there (`FILE_OPEN_REPARSE_POINT`) and without following
//!   a link anywhere else (`OBJ_DONT_REPARSE`). A sub-folder that is a link is refused.
//! - A file is put in place by renaming its open handle to a name in the folder's
//!   handle, and a temporary file is removed through its handle.
//! - The folder handle does not share delete access, so the folder cannot be renamed
//!   or removed while it is open. NTFS refuses to rename a folder with an open handle
//!   below it, so the folders above are held in place too, up to the drive.
//!
//! An empty folder can still be made a junction in place while it is open (that needs
//! only the right to write its attributes). An open by name relative to it is then
//! refused, so nothing is made or read where the junction points.
//!
//! The store's own folder is opened by its path and may itself be a link: the owner
//! chose it. Once it is open, a link above it pointed elsewhere changes nothing.
//!
//! Elsewhere than Windows the path is checked and then used, as before, with the gap
//! between the two.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

/// The error of a link found where a plain folder or file was expected.
#[derive(Debug)]
struct Linked;

impl fmt::Display for Linked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("링크")
    }
}

impl std::error::Error for Linked {}

fn linked() -> io::Error {
    io::Error::other(Linked)
}

/// Whether `e` is the refusal of a link.
pub(crate) fn is_linked(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<Linked>())
}

/// How a file of a folder is opened.
#[derive(Clone, Copy)]
pub(crate) enum Open {
    /// Read; the file must be there.
    Read,
    /// Read and write, made if missing (a lock file).
    Lock,
    /// Read, and write at the end only; made if missing (a log).
    Append,
}

/// A plain folder of the document store, held open. `path` is where it was opened, for
/// messages only.
///
/// One operation opens the store once and finds every folder it uses from that, so
/// all it reads and writes is in the one store it opened, whatever a link above the
/// store points at meanwhile.
pub(crate) struct Folder {
    handle: fs::File,
    path: PathBuf,
    /// Elsewhere than Windows: the folders opened below the store, outermost first, each
    /// checked again before every use.
    #[cfg(not(windows))]
    chain: Vec<PathBuf>,
}

/// One name in a folder: never a path.
fn check_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("폴더 안의 이름이 아닙니다: {name}"),
        ));
    }
    Ok(())
}

fn found<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

impl Folder {
    /// The store `vault`, opened by its path: `None` when it is not there.
    pub(crate) fn store(vault: &Path) -> io::Result<Option<Folder>> {
        found(Folder::open_store(vault))
    }

    /// The folder `names` joined below the store `vault`, made where missing, the store
    /// too. A link on the way is an error (`is_linked`).
    pub(crate) fn made(vault: &Path, names: &[&str]) -> io::Result<Folder> {
        fs::create_dir_all(vault)?;
        Folder::open_store(vault)?.made_below(names)
    }

    /// The folder `names` joined below the store `vault`: `None` when the store or one
    /// of the folders is not there. Nothing is made. A link on the way is an error.
    pub(crate) fn found(vault: &Path, names: &[&str]) -> io::Result<Option<Folder>> {
        match Folder::store(vault)? {
            Some(store) => store.below(names),
            None => Ok(None),
        }
    }

    /// The folder `names` joined below this one: `None` when one of them is not there.
    pub(crate) fn below(&self, names: &[&str]) -> io::Result<Option<Folder>> {
        let mut folder: Option<Folder> = None;
        for name in names {
            let sub = folder.as_ref().unwrap_or(self).find(name)?;
            match sub {
                Some(sub) => folder = Some(sub),
                None => return Ok(None),
            }
        }
        match folder {
            Some(folder) => Ok(Some(folder)),
            None => self.again().map(Some),
        }
    }

    /// The folder `names` joined below this one, made where missing.
    pub(crate) fn made_below(&self, names: &[&str]) -> io::Result<Folder> {
        let mut folder = self.again()?;
        for name in names {
            folder = folder.make(name)?;
        }
        Ok(folder)
    }

    /// The path of `name` in this folder, for messages.
    pub(crate) fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    fn open_store(vault: &Path) -> io::Result<Folder> {
        let handle = sys::open_store(vault)?;
        if !handle.metadata()?.is_dir() {
            return Err(io::Error::other(format!(
                "문서 저장소({})가 폴더가 아닙니다.",
                vault.display()
            )));
        }
        Ok(Folder {
            handle,
            path: vault.to_path_buf(),
            #[cfg(not(windows))]
            chain: Vec::new(),
        })
    }

    /// This folder once more, as a value of its own.
    fn again(&self) -> io::Result<Folder> {
        Ok(Folder {
            handle: self.handle.try_clone()?,
            path: self.path.clone(),
            #[cfg(not(windows))]
            chain: self.chain.clone(),
        })
    }

    /// Elsewhere than Windows nothing holds the folders, so before each use every folder
    /// opened below the store is checked again to be a plain folder.
    fn recheck(&self) -> io::Result<()> {
        #[cfg(not(windows))]
        for folder in &self.chain {
            let meta = fs::symlink_metadata(folder)?;
            if meta.file_type().is_symlink() {
                return Err(linked());
            }
            if !meta.is_dir() {
                return Err(io::Error::other(format!(
                    "{}이 폴더가 아닙니다.",
                    folder.display()
                )));
            }
        }
        Ok(())
    }

    fn sub(&self, name: &str, make: bool) -> io::Result<Folder> {
        check_name(name)?;
        self.recheck()?;
        let handle = sys::open_folder(&self.handle, &self.file(name), name, make)?;
        let meta = handle.metadata()?;
        if meta.file_type().is_symlink() {
            return Err(linked());
        }
        if !meta.is_dir() {
            return Err(io::Error::other(format!(
                "{}이 폴더가 아닙니다.",
                self.file(name).display()
            )));
        }
        #[cfg(not(windows))]
        let chain = {
            let mut chain = self.chain.clone();
            chain.push(self.file(name));
            chain
        };
        Ok(Folder {
            handle,
            path: self.file(name),
            #[cfg(not(windows))]
            chain,
        })
    }

    /// The plain sub-folder `name`, or `None` when there is none. A link there is an
    /// error (`is_linked`).
    pub(crate) fn find(&self, name: &str) -> io::Result<Option<Folder>> {
        found(self.sub(name, false))
    }

    /// The plain sub-folder `name`, made if missing.
    pub(crate) fn make(&self, name: &str) -> io::Result<Folder> {
        self.sub(name, true)
    }

    /// The file `name`, opened `how`. A link there is an error (`is_linked`), and so is
    /// a folder.
    pub(crate) fn open(&self, name: &str, how: Open) -> io::Result<fs::File> {
        check_name(name)?;
        self.recheck()?;
        let file = sys::open_file(&self.handle, &self.file(name), name, how)?;
        plain_file(&file, &self.file(name))?;
        Ok(file)
    }

    /// The bytes of the file `name`.
    pub(crate) fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        io::Read::read_to_end(&mut self.open(name, Open::Read)?, &mut bytes)?;
        Ok(bytes)
    }

    /// The names in this folder, folders and links included.
    pub(crate) fn names(&self) -> io::Result<Vec<String>> {
        self.recheck()?;
        sys::names(&self.handle, &self.path)
    }

    /// Writes `bytes` whole to a new temporary file in this folder and puts it in place
    /// as `name`, replacing a file there. A temporary file left by a failure is removed.
    pub(crate) fn publish(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        check_name(name)?;
        self.recheck()?;
        let temp = format!(".{}.tmp", uuid::Uuid::new_v4());
        let mut file = sys::create_new(&self.handle, &self.file(&temp), &temp)?;
        let result = (|| {
            plain_file(&file, &self.file(&temp))?;
            io::Write::write_all(&mut file, bytes)?;
            file.sync_all()?;
            self.recheck()?;
            sys::rename(
                &file,
                &self.file(&temp),
                &self.handle,
                &self.file(name),
                name,
            )
        })();
        if let Err(e) = &result {
            // On Windows the file is removed through its own handle. Elsewhere it is
            // removed by path, so not after a link was found on the way, even if the
            // folders look plain again: that path may now name a file somewhere else.
            let by_path_elsewhere = cfg!(not(windows)) && (is_linked(e) || self.recheck().is_err());
            if !by_path_elsewhere {
                let _ = sys::remove(file, &self.file(&temp));
            }
        }
        result
    }
}

fn plain_file(file: &fs::File, path: &Path) -> io::Result<()> {
    let meta = file.metadata()?;
    if meta.file_type().is_symlink() {
        return Err(linked());
    }
    if meta.is_dir() {
        return Err(io::Error::other(format!(
            "{}이 파일이 아니라 폴더입니다.",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(windows)]
mod sys {
    use super::{linked, Open};
    use std::{
        fs, io,
        os::windows::{
            fs::OpenOptionsExt,
            io::{AsRawHandle, FromRawHandle},
        },
        path::Path,
        ptr,
    };
    use windows_sys::{
        Wdk::{
            Foundation::OBJECT_ATTRIBUTES,
            Storage::FileSystem::{
                FileDispositionInformation, FileRenameInformation, FileRenameInformationEx,
                NtCreateFile, NtSetInformationFile, FILE_CREATE, FILE_DIRECTORY_FILE,
                FILE_DISPOSITION_INFORMATION, FILE_INFORMATION_CLASS, FILE_NON_DIRECTORY_FILE,
                FILE_OPEN, FILE_OPEN_IF, FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION,
                FILE_SYNCHRONOUS_IO_NONALERT,
            },
        },
        Win32::{
            Foundation::{
                RtlNtStatusToDosError, ERROR_NO_MORE_FILES, HANDLE, NTSTATUS, OBJ_CASE_INSENSITIVE,
                OBJ_DONT_REPARSE, STATUS_REPARSE_POINT_ENCOUNTERED, UNICODE_STRING,
            },
            Storage::FileSystem::{
                FileFullDirectoryInfo, FileFullDirectoryRestartInfo, GetFileInformationByHandleEx,
                DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_BACKUP_SEMANTICS,
                FILE_FULL_DIR_INFO, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_LIST_DIRECTORY,
                FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
                FILE_TRAVERSE, FILE_WRITE_DATA, SYNCHRONIZE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };

    /// `STATUS_REPARSE_POINT_NOT_RESOLVED`: what an open relative to a folder that was
    /// made a junction after it was opened returns with `OBJ_DONT_REPARSE`.
    const STATUS_REPARSE_POINT_NOT_RESOLVED: NTSTATUS = 0xC000_0280_u32 as NTSTATUS;

    /// What the folders are shared with: reading and writing, not deleting, so a folder
    /// held open cannot be renamed or removed.
    const FOLDER_SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;
    /// What files are shared with, as `std::fs` shares them.
    const FILE_SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

    fn handle(file: &fs::File) -> HANDLE {
        file.as_raw_handle() as HANDLE
    }

    fn with_path(e: io::Error, path: &Path) -> io::Error {
        if super::is_linked(&e) {
            return e;
        }
        io::Error::new(e.kind(), format!("{}: {e}", path.display()))
    }

    fn error(status: NTSTATUS) -> io::Error {
        if status == STATUS_REPARSE_POINT_ENCOUNTERED || status == STATUS_REPARSE_POINT_NOT_RESOLVED
        {
            return linked();
        }
        // SAFETY: a plain conversion of a status code.
        io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32)
    }

    pub(super) fn open_store(vault: &Path) -> io::Result<fs::File> {
        fs::OpenOptions::new()
            .read(true)
            .share_mode(FOLDER_SHARE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(vault)
            .map_err(|e| with_path(e, vault))
    }

    /// Opens `name` relative to the folder `dir`, never through a link.
    fn open_at(
        dir: &fs::File,
        name: &str,
        access: u32,
        share: u32,
        disposition: u32,
        options: u32,
    ) -> io::Result<fs::File> {
        let wide: Vec<u16> = name.encode_utf16().collect();
        let bytes = u16::try_from(wide.len() * 2)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "이름이 너무 깁니다."))?;
        let object = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: wide.as_ptr() as *mut u16,
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: handle(dir),
            ObjectName: &object,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            SecurityDescriptor: ptr::null(),
            SecurityQualityOfService: ptr::null(),
        };
        let mut opened: HANDLE = ptr::null_mut();
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: every pointer is to a live local value for the length of the call, and
        // `dir` stays open for it.
        let status = unsafe {
            NtCreateFile(
                &mut opened,
                access | SYNCHRONIZE,
                &attributes,
                &mut status_block,
                ptr::null(),
                FILE_ATTRIBUTE_NORMAL,
                share,
                disposition,
                options | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
                ptr::null(),
                0,
            )
        };
        if status < 0 {
            return Err(error(status));
        }
        // SAFETY: `opened` is a new handle that nothing else owns.
        Ok(unsafe { fs::File::from_raw_handle(opened as _) })
    }

    pub(super) fn open_folder(
        dir: &fs::File,
        path: &Path,
        name: &str,
        make: bool,
    ) -> io::Result<fs::File> {
        open_at(
            dir,
            name,
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES,
            FOLDER_SHARE,
            if make { FILE_OPEN_IF } else { FILE_OPEN },
            FILE_DIRECTORY_FILE,
        )
        .map_err(|e| with_path(e, path))
    }

    pub(super) fn open_file(
        dir: &fs::File,
        path: &Path,
        name: &str,
        how: Open,
    ) -> io::Result<fs::File> {
        let (access, disposition) = match how {
            Open::Read => (FILE_GENERIC_READ, FILE_OPEN),
            Open::Lock => (FILE_GENERIC_READ | FILE_GENERIC_WRITE, FILE_OPEN_IF),
            // Without `FILE_WRITE_DATA` every write goes to the end, as `append` does.
            Open::Append => (
                FILE_GENERIC_READ | (FILE_GENERIC_WRITE & !FILE_WRITE_DATA) | FILE_APPEND_DATA,
                FILE_OPEN_IF,
            ),
        };
        // No `FILE_NON_DIRECTORY_FILE`: a junction in place of the file is then opened
        // itself and refused as a link, not reported as a folder.
        open_at(dir, name, access, FILE_SHARE, disposition, 0).map_err(|e| with_path(e, path))
    }

    pub(super) fn create_new(dir: &fs::File, path: &Path, name: &str) -> io::Result<fs::File> {
        open_at(
            dir,
            name,
            FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE,
            FILE_SHARE,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE,
        )
        .map_err(|e| with_path(e, path))
    }

    /// `FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS`: a file there is
    /// replaced even while another handle that shares delete access reads it, as
    /// `std::fs::rename` does on NTFS.
    const POSIX_REPLACE: u32 = 0x1 | 0x2;
    const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000_000D_u32 as NTSTATUS;
    const STATUS_INVALID_INFO_CLASS: NTSTATUS = 0xC000_0003_u32 as NTSTATUS;
    const STATUS_NOT_SUPPORTED: NTSTATUS = 0xC000_00BB_u32 as NTSTATUS;
    const STATUS_NOT_IMPLEMENTED: NTSTATUS = 0xC000_0002_u32 as NTSTATUS;

    /// Gives the open `file` the name `name` in the folder `dir`, replacing a file there.
    /// A file system that has no POSIX rename gets the plain one.
    pub(super) fn rename(
        file: &fs::File,
        from: &Path,
        dir: &fs::File,
        to: &Path,
        name: &str,
    ) -> io::Result<()> {
        let mut status = rename_as(file, dir, name, FileRenameInformationEx, POSIX_REPLACE);
        if matches!(
            status,
            STATUS_INVALID_PARAMETER
                | STATUS_INVALID_INFO_CLASS
                | STATUS_NOT_SUPPORTED
                | STATUS_NOT_IMPLEMENTED
        ) {
            status = rename_as(file, dir, name, FileRenameInformation, 0);
        }
        if status < 0 {
            let e = error(status);
            return Err(io::Error::new(
                e.kind(),
                format!("{} → {}: {e}", from.display(), to.display()),
            ));
        }
        Ok(())
    }

    /// One rename call of the class `class`. With `FileRenameInformation` the first field
    /// is `ReplaceIfExists`; with `FileRenameInformationEx` it is `flags`.
    fn rename_as(
        file: &fs::File,
        dir: &fs::File,
        name: &str,
        class: FILE_INFORMATION_CLASS,
        flags: u32,
    ) -> NTSTATUS {
        let wide: Vec<u16> = name.encode_utf16().collect();
        let offset = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
        // The whole structure and the name, as the documentation asks: never less than
        // the structure, though the name starts inside it.
        let length = size_of::<FILE_RENAME_INFORMATION>() + wide.len() * 2;
        // u64 words keep the buffer aligned for the structure.
        let mut buffer = vec![0_u64; length.div_ceil(8)];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        // SAFETY: the buffer is aligned and longer than the structure and the name after
        // its name field; the name is written within it.
        unsafe {
            if class == FileRenameInformation {
                (*info).Anonymous.ReplaceIfExists = true;
            } else {
                (*info).Anonymous.Flags = flags;
            }
            (*info).RootDirectory = handle(dir);
            (*info).FileNameLength = (wide.len() * 2) as u32;
            ptr::copy_nonoverlapping(
                wide.as_ptr(),
                info.cast::<u8>().add(offset).cast::<u16>(),
                wide.len(),
            );
        }
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: the buffer outlives the call, and both handles are open.
        unsafe {
            NtSetInformationFile(
                handle(file),
                &mut status_block,
                info.cast(),
                length as u32,
                class,
            )
        }
    }

    /// Removes the open `file`: it goes once the handle is closed here.
    pub(super) fn remove(file: fs::File, path: &Path) -> io::Result<()> {
        let info = FILE_DISPOSITION_INFORMATION { DeleteFile: true };
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: `info` outlives the call, and the handle is open.
        let status = unsafe {
            NtSetInformationFile(
                handle(&file),
                &mut status_block,
                (&info as *const FILE_DISPOSITION_INFORMATION).cast(),
                size_of::<FILE_DISPOSITION_INFORMATION>() as u32,
                FileDispositionInformation,
            )
        };
        drop(file);
        if status < 0 {
            return Err(with_path(error(status), path));
        }
        Ok(())
    }

    pub(super) fn names(dir: &fs::File, path: &Path) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        // u64 words keep the buffer aligned for the entries.
        let mut buffer = vec![0_u64; 8192];
        let mut class = FileFullDirectoryRestartInfo;
        loop {
            // SAFETY: the buffer outlives the call and its length is given in bytes.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    handle(dir),
                    class,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 8) as u32,
                )
            };
            if ok == 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    return Ok(names);
                }
                return Err(with_path(e, path));
            }
            class = FileFullDirectoryInfo;
            let start = buffer.as_ptr().cast::<u8>();
            let mut at = 0_usize;
            loop {
                // SAFETY: the system wrote whole entries into the buffer, each at the
                // offset the one before it gives, and each entry starts aligned.
                let entry = unsafe { &*start.add(at).cast::<FILE_FULL_DIR_INFO>() };
                let name = unsafe {
                    std::slice::from_raw_parts(
                        start
                            .add(at + std::mem::offset_of!(FILE_FULL_DIR_INFO, FileName))
                            .cast::<u16>(),
                        entry.FileNameLength as usize / 2,
                    )
                };
                let name = String::from_utf16_lossy(name);
                if name != "." && name != ".." {
                    names.push(name);
                }
                if entry.NextEntryOffset == 0 {
                    break;
                }
                at += entry.NextEntryOffset as usize;
            }
        }
    }
}

/// Elsewhere than Windows the path is checked, then used.
#[cfg(not(windows))]
mod sys {
    use super::{linked, Open};
    use crate::chat_originals::is_link;
    use std::{fs, io, path::Path};

    fn plain(path: &Path) -> io::Result<()> {
        if is_link(path)? {
            return Err(linked());
        }
        Ok(())
    }

    /// Only a folder is opened as one: opening anything else (a FIFO, say) could wait.
    fn folder(path: &Path) -> io::Result<fs::File> {
        if !fs::metadata(path)?.is_dir() {
            return Err(io::Error::other(format!(
                "{}이 폴더가 아닙니다.",
                path.display()
            )));
        }
        fs::File::open(path)
    }

    pub(super) fn open_store(vault: &Path) -> io::Result<fs::File> {
        folder(vault)
    }

    pub(super) fn open_folder(
        _dir: &fs::File,
        path: &Path,
        _name: &str,
        make: bool,
    ) -> io::Result<fs::File> {
        plain(path)?;
        if make {
            match fs::create_dir(path) {
                Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
                _ => {}
            }
        }
        plain(path)?;
        folder(path)
    }

    pub(super) fn open_file(
        _dir: &fs::File,
        path: &Path,
        _name: &str,
        how: Open,
    ) -> io::Result<fs::File> {
        plain(path)?;
        let mut options = fs::OpenOptions::new();
        match how {
            Open::Read => options.read(true),
            Open::Lock => options.read(true).write(true).create(true).truncate(false),
            Open::Append => options.read(true).append(true).create(true),
        };
        options.open(path)
    }

    pub(super) fn create_new(_dir: &fs::File, path: &Path, _name: &str) -> io::Result<fs::File> {
        fs::File::create_new(path)
    }

    pub(super) fn rename(
        _file: &fs::File,
        from: &Path,
        _dir: &fs::File,
        to: &Path,
        _name: &str,
    ) -> io::Result<()> {
        fs::rename(from, to)
    }

    pub(super) fn remove(_file: fs::File, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    pub(super) fn names(_dir: &fs::File, path: &Path) -> io::Result<Vec<String>> {
        fs::read_dir(path)?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect()
    }
}

#[cfg(test)]
mod tests;
