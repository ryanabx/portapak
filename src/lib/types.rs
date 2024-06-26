use std::{env, fs::remove_dir_all, path::PathBuf};

use libflatpak::{
    gio::prelude::FileExt,
    prelude::{InstallationExt, RemoteExt},
    BundleRef, RefKind,
};
use rand::{distributions::Alphanumeric, thread_rng, Rng};

#[derive(Debug)]
pub enum FlatpakExtError {
    Glib(libflatpak::glib::Error),
    IO(std::io::Error),
    Reqwest(reqwest::Error),
}

impl From<std::io::Error> for FlatpakExtError {
    fn from(value: std::io::Error) -> Self {
        Self::IO(value)
    }
}

impl From<libflatpak::glib::Error> for FlatpakExtError {
    fn from(value: libflatpak::glib::Error) -> Self {
        Self::Glib(value)
    }
}

impl From<reqwest::Error> for FlatpakExtError {
    fn from(value: reqwest::Error) -> Self {
        Self::Reqwest(value)
    }
}

#[derive(Clone, Debug)]
pub enum Flatpak {
    Bundle(PathBuf),
    Download(String),
}

#[derive(Clone, Debug)]
pub enum FlatpakOut {
    Bundle(libflatpak::BundleRef),
    Download(libflatpak::RemoteRef),
}

impl Flatpak {
    pub fn convert_to_flatpak_out(
        &self,
        installation: &libflatpak::Installation,
        remote: &libflatpak::Remote,
        branch: &String,
        is_runtime: bool,
    ) -> Result<FlatpakOut, FlatpakExtError> {
        match self {
            Flatpak::Bundle(path) => {
                let bundle_path = libflatpak::gio::File::for_path(&path);
                let bundle = BundleRef::new(&bundle_path)?;
                Ok(FlatpakOut::Bundle(bundle))
            }
            Flatpak::Download(app_id) => {
                Ok(FlatpakOut::Download(installation.fetch_remote_ref_sync(
                    &remote.name().unwrap(),
                    if is_runtime {
                        RefKind::Runtime
                    } else {
                        RefKind::App
                    },
                    &app_id,
                    libflatpak::default_arch().as_deref(),
                    Some(&branch),
                    libflatpak::gio::Cancellable::current().as_ref(),
                )?))
            }
        }
    }
}

#[derive(Clone, Debug)]
/// A remote to download from
pub struct Remote {
    /// uri to a .flatpakrepo file (can be a URL or a file path)
    uri: String,
    name: String,
    pub default_branch: String,
}

impl Default for Remote {
    fn default() -> Self {
        Remote {
            uri: "https://dl.flathub.org/repo/flathub.flatpakrepo".into(),
            name: "flathub".into(),
            default_branch: "stable".into(),
        }
    }
}

impl Remote {
    pub fn new(uri: String) -> Self {
        Remote {
            uri: uri.clone(),
            name: uri.clone(),
            default_branch: "master".into(),
        }
    }
}

impl TryFrom<Remote> for libflatpak::Remote {
    fn try_from(value: Remote) -> Result<Self, Self::Error> {
        log::trace!("Loading bytes from uri: '{}'", value.uri);
        let bytes = uri_to_bytes(value.uri)?;
        let remote = libflatpak::Remote::from_file(&value.name, &bytes)?;
        if remote.name().unwrap().to_string() == "flathub".to_string() {
            remote.set_default_branch("stable");
        }
        Ok(remote)
    }

    type Error = FlatpakExtError;
}

pub fn uri_to_bytes(uri: String) -> Result<libflatpak::glib::Bytes, FlatpakExtError> {
    if uri.starts_with("file://") {
        Ok(
            libflatpak::gio::File::for_path(&uri.split_once("file://").unwrap().0)
                .load_bytes(libflatpak::gio::Cancellable::current().as_ref())?
                .0,
        )
    } else {
        Ok(libflatpak::glib::Bytes::from_owned(
            reqwest::blocking::get(uri)?.bytes().unwrap(),
        ))
    }
}

#[derive(Clone, Debug, Default)]
/// An abstraction representing a flatpak repository
pub enum Repo {
    /// A temporary repo that is intended to be deleted when the value is dropped
    /// > **WARNING:** Only enter in a directory that should be deleted when dropped
    Temp(PathBuf),
    /// The default System repo
    #[default]
    System,
    /// The default User repo
    User,
    /// A static repo that will persist after the execution of this program
    /// `user`=`true` for `--user`, `user`=`false` for `--system`
    Static { path: PathBuf, user: bool },
}

impl Repo {
    /// Creates a new temp repo in the default tmp directory
    pub fn temp() -> Self {
        let path = env::temp_dir();
        Self::temp_in(path)
    }

    /// Creates a new temp repo in the specified directory
    pub fn temp_in(path: PathBuf) -> Self {
        let foldername: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();
        let temp_repo = path.join(format!(".tmp{}", foldername));
        Repo::Temp(temp_repo)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        if let Self::Temp(path) = self {
            log::debug!("Dropping TempRepo: {}", &path.to_string_lossy());
            let _ = remove_dir_all(&path);
        }
    }
}

pub fn get_installation(value: &Repo) -> Result<libflatpak::Installation, FlatpakExtError> {
    match value {
        Repo::Temp(ref path) => {
            let repo_file = libflatpak::gio::File::for_path(path);
            // Create installation
            Ok(libflatpak::Installation::for_path(
                &repo_file,
                true,
                libflatpak::gio::Cancellable::current().as_ref(),
            )?)
        }
        Repo::Static { ref path, user } => {
            let repo_file = libflatpak::gio::File::for_path(path);
            Ok(libflatpak::Installation::for_path(
                &repo_file,
                *user,
                libflatpak::gio::Cancellable::current().as_ref(),
            )?)
        }
        Repo::System => Ok(libflatpak::Installation::new_system(
            libflatpak::gio::Cancellable::current().as_ref(),
        )?),
        Repo::User => Ok(libflatpak::Installation::new_user(
            libflatpak::gio::Cancellable::current().as_ref(),
        )?),
    }
}
