//! Operating-system differences in one place: what each platform can promise
//! about private files, durable renames, executables and long-lived processes.
//! Callers use these instead of their own `cfg` blocks.
use std::fs;
use std::io;
use std::path::Path;

/// Temporary directories readable only by the current user where the
/// platform has Unix modes; elsewhere the user profile's access control applies.
pub(crate) fn private_tempdir(builder: &mut tempfile::Builder<'_, '_>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = builder;
}

/// Directories readable only by the current user, as for `private_tempdir`.
pub(crate) fn private_directory(builder: &mut fs::DirBuilder) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    #[cfg(not(unix))]
    let _ = builder;
}

/// Files readable only by the current user, as for `private_tempdir`.
pub(crate) fn private_file(options: &mut fs::OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = options;
}

/// Persist a directory's entries, such as a rename into it. Windows cannot
/// open a directory to sync it: its renames stay atomic for other processes
/// but are not ordered against power loss, so callers must not claim that.
pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(path).and_then(|file| file.sync_all())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Flush a file's contents to disk. Windows flushes only through a handle
/// opened for writing; the file's bytes are not changed.
pub(crate) fn sync_file(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    let file = fs::OpenOptions::new().write(true).open(path)?;
    #[cfg(not(windows))]
    let file = fs::File::open(path)?;
    file.sync_all()
}

/// Whether a regular file can be run: an execute bit on Unix; on Windows the
/// extension decides, which PATH lookups apply through PATHEXT.
pub(crate) fn is_executable(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// The executable a bare command name resolves to in `path` (a PATH value),
/// relative entries taken from `base`. Windows also tries each `PATHEXT`
/// extension, which is how `claude` finds npm's `claude.cmd`.
pub(crate) fn find_program(
    name: &str,
    path: &std::ffi::OsStr,
    base: &Path,
) -> Option<std::path::PathBuf> {
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_owned())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(path)
        .flat_map(|directory| {
            let directory = base.join(directory);
            extensions
                .iter()
                .map(move |extension| directory.join(format!("{name}{extension}")))
        })
        .find(|candidate| fs::metadata(candidate).is_ok_and(|metadata| is_executable(&metadata)))
}

/// The local path a `file://` URI names, as editors record workspaces.
/// Windows converts only drive paths (`file:///c%3A/...`) natively, so a
/// drive-less URI (a WSL or remote workspace) keeps its decoded POSIX path
/// there instead of being dropped; elsewhere the standard conversion applies.
pub(crate) fn file_uri_path(uri: &str) -> Option<String> {
    let url = reqwest::Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    if let Ok(path) = url.to_file_path() {
        return path.to_str().map(str::to_owned);
    }
    if !cfg!(windows) || url.host_str().is_some_and(|host| !host.is_empty()) {
        return None;
    }
    let path = percent_decode(url.path())?;
    // An encoded drive (`/c%3A/...`) decodes to `/c:/...`: a native path.
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return Some(path[1..].replace('/', "\\"));
    }
    Some(path)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// Set on a long-lived Windows process started by `run_from_copy`: the
/// installed executable it stands in for.
pub(crate) const COPY_OF: &str = "MASH_COPY_OF";

/// Windows cannot replace a running `.exe`, so a long-lived command (MCP,
/// `index --watch`, `serve`) would make every upgrade fail. It instead runs
/// from a copy kept per installed build under the local data directory, and
/// the runtime lease watches the installed path, so an upgrade still stops new
/// writes. Returns the copy's exit code, or None to run in place (on other
/// platforms, inside the copy, or when the copy cannot be made).
#[cfg(windows)]
pub(crate) fn run_from_copy(arguments: &[std::ffi::OsString]) -> Option<i32> {
    if std::env::var_os(COPY_OF).is_some() {
        return None;
    }
    let installed = std::env::current_exe().ok()?;
    let metadata = fs::metadata(&installed).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let build = format!("{}-{modified}", metadata.len());
    let root = directories::ProjectDirs::from("dev", "jamesqo", "mash")?
        .data_local_dir()
        .join("run");
    let copy = root.join(&build).join("mash.exe");
    if !copy.is_file() {
        fs::create_dir_all(root.join(&build)).ok()?;
        let staged = root.join(&build).join("mash.exe.partial");
        fs::copy(&installed, &staged).ok()?;
        fs::rename(&staged, &copy).ok()?;
    }
    // Copies of earlier builds go; one still running is locked and stays.
    for entry in fs::read_dir(&root).into_iter().flatten().flatten() {
        if entry.file_name() != build.as_str() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    let status = std::process::Command::new(&copy)
        .args(arguments.iter().skip(1))
        .env(COPY_OF, &installed)
        .status()
        .ok()?;
    Some(status.code().unwrap_or(1))
}

#[cfg(not(windows))]
pub(crate) fn run_from_copy(_arguments: &[std::ffi::OsString]) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::file_uri_path;

    #[test]
    fn file_uris_name_local_paths() {
        assert_eq!(file_uri_path("https://example.com/x"), None);
        assert_eq!(file_uri_path("not a uri"), None);
        if cfg!(windows) {
            assert_eq!(
                file_uri_path("file:///c%3A/Users/Ada/repo").as_deref(),
                Some("c:\\Users\\Ada\\repo")
            );
            assert_eq!(
                file_uri_path("file:///work/my%20repo").as_deref(),
                Some("/work/my repo")
            );
        } else {
            assert_eq!(
                file_uri_path("file:///work/my%20repo").as_deref(),
                Some("/work/my repo")
            );
        }
    }
}
