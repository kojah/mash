//! Operating-system differences in one place: what each platform can promise
//! about private files, durable renames, executables and long-lived processes.
//! Callers use these instead of their own `cfg` blocks.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Temporary directories readable only by the current user where the
/// platform has Unix modes; elsewhere the user profile's access control applies.
pub fn private_tempdir(builder: &mut tempfile::Builder<'_, '_>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = builder;
}

/// Directories readable only by the current user, as for `private_tempdir`.
pub fn private_directory(builder: &mut fs::DirBuilder) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    #[cfg(not(unix))]
    let _ = builder;
}

/// Files readable only by the current user, as for `private_tempdir`.
pub fn private_file(options: &mut fs::OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = options;
}

/// Mash's own directory, `~/.mash`, on every platform: its configuration,
/// index, models and Qdrant all live here, next to the agents' own folders.
pub fn mash_home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|directories| directories.home_dir().join(".mash"))
}

/// Persist a directory's entries, such as a rename into it. Windows cannot
/// open a directory to sync it: its renames stay atomic for other processes
/// but are not ordered against power loss, so callers must not claim that.
pub fn sync_directory(path: &Path) -> io::Result<()> {
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
pub fn sync_file(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    let file = fs::OpenOptions::new().write(true).open(path)?;
    #[cfg(not(windows))]
    let file = fs::File::open(path)?;
    file.sync_all()
}

/// Whether a regular file can be run: an execute bit on Unix; on Windows the
/// extension decides, which PATH lookups apply through PATHEXT.
pub fn is_executable(metadata: &fs::Metadata) -> bool {
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
pub fn find_program(name: &str, path: &std::ffi::OsStr, base: &Path) -> Option<std::path::PathBuf> {
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
pub fn file_uri_path(uri: &str) -> Option<String> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    if let Ok(path) = url.to_file_path() {
        return path.to_str().map(upper_drive);
    }
    if !cfg!(windows) || url.host_str().is_some_and(|host| !host.is_empty()) {
        return None;
    }
    uri_path(&percent_decode(url.path())?)
}

/// The path a VS Code remote workspace URI names on its remote machine
/// (`vscode-remote://ssh-remote%2Bhost/home/me/repo` is `/home/me/repo`).
/// The authority identifies the SSH host, container or Codespace instead.
pub fn remote_uri_path(uri: &str) -> Option<String> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "vscode-remote" || url.host_str().is_none_or(str::is_empty) {
        return None;
    }
    uri_path(&percent_decode(url.path())?)
}

/// The local path for a decoded URI path, as structured editor URIs carry
/// it: always `/`-rooted. On Windows a drive path (`/c:/...`) becomes native
/// (`C:\...`); any other rooted path keeps its POSIX form.
pub fn uri_path(path: &str) -> Option<String> {
    if !path.starts_with('/') {
        return None;
    }
    if cfg!(windows) && is_drive_path(&path[1..]) {
        return Some(upper_drive(&path[1..].replace('/', "\\")));
    }
    Some(path.to_owned())
}

/// Whether `path` starts with a Windows drive (`C:\` or `c:/`), whichever
/// platform reads it: histories written on Windows can be read elsewhere.
pub fn is_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

/// Editors record workspace URIs with a lowercase drive (`file:///c%3A/...`)
/// while agents record the process's working directory (`C:\...`). Drive
/// letters are case-insensitive, so one spelling keeps a folder one project.
fn upper_drive(path: &str) -> String {
    if is_drive_path(path) {
        let mut path = path.to_owned();
        path[..1].make_ascii_uppercase();
        return path;
    }
    path.to_owned()
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

/// Windows cannot replace a running `.exe`, so a long-lived command (MCP,
/// `index --watch`, `mash-server`) would make every upgrade of the installed file
/// fail. It may rename itself, though: the process moves its executable aside
/// (`.mash-running-<pid>.exe`, removed by a later start once no longer
/// running) and restores an ordinary copy at the installed path for installers
/// to replace. The runtime lease keeps watching the installed path, so an
/// upgrade still stops new writes. A directory the user cannot write (an
/// administrator install) keeps running in place. Between the two renames the
/// installed path is briefly absent, so `MASH_KEEP_EXECUTABLE=1` opts out
/// where processes start constantly (test runners). No-op elsewhere.
pub fn release_installed_executable() {
    #[cfg(windows)]
    {
        if std::env::var_os("MASH_KEEP_EXECUTABLE").is_some_and(|value| value == "1") {
            return;
        }
        let Ok(installed) = std::env::current_exe() else {
            return;
        };
        let Some(directory) = installed.parent() else {
            return;
        };
        for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(".mash-running-")
            {
                let _ = fs::remove_file(entry.path()); // Running ones stay locked.
            }
        }
        let id = std::process::id();
        let moved = directory.join(format!(".mash-running-{id}.exe"));
        let staged = directory.join(format!(".mash-staged-{id}.exe"));
        if fs::copy(&installed, &staged).is_err() {
            let _ = fs::remove_file(&staged);
            return;
        }
        if fs::rename(&installed, &moved).is_err() {
            let _ = fs::remove_file(&staged);
            return;
        }
        if fs::rename(&staged, &installed).is_err() {
            // Put the running file back rather than leave no executable.
            let _ = fs::rename(&moved, &installed);
            let _ = fs::remove_file(&staged);
        }
    }
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
                Some("C:\\Users\\Ada\\repo")
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

    #[test]
    fn remote_workspace_uris_name_their_remote_paths() {
        use super::remote_uri_path;
        for (uri, path) in [
            (
                "vscode-remote://ssh-remote%2Bbuild-host/home/ada/scratch/lockfile",
                "/home/ada/scratch/lockfile",
            ),
            (
                "vscode-remote://codespaces%2Bfriendly-space-tribble-1234/workspaces/my%20repo",
                "/workspaces/my repo",
            ),
        ] {
            assert_eq!(remote_uri_path(uri).as_deref(), Some(path), "{uri}");
        }
        for uri in [
            "file:///home/ada/repo",
            "vscode-remote:///home/ada/repo",
            "vscode-remote://ssh-remote%2Bhost",
            "vscode-vfs://github/ada/repo",
            "not a uri",
        ] {
            assert_eq!(remote_uri_path(uri), None, "{uri}");
        }
    }

    #[test]
    fn drive_paths_are_recognized_and_spelled_one_way_on_every_platform() {
        use super::{is_drive_path, upper_drive};
        for path in ["C:\\Users\\Ada", "c:/Users/Ada", "d:\\"] {
            assert!(is_drive_path(path), "{path}");
        }
        for path in [
            "/home/ada",
            "C:",
            "C:relative",
            "\\\\server\\share",
            "1:\\x",
            "",
        ] {
            assert!(!is_drive_path(path), "{path}");
        }
        assert_eq!(upper_drive("c:\\Users\\Ada"), "C:\\Users\\Ada");
        assert_eq!(upper_drive("/c:/Users"), "/c:/Users");
        assert_eq!(upper_drive("/home/ada"), "/home/ada");
    }
}
