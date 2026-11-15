use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub target: String,
    pub folder: String,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            target: String::new(),
            folder: "~/screenshots".into(),
        }
    }
}
impl Host {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Give this host a name.".into());
        }
        let t = &self.target;
        if t.is_empty()
            || t.starts_with('-')
            || !t
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "@._-:[]".contains(c))
            || t.matches('@').count() > 1
        {
            return Err("Enter an SSH alias or user@hostname, without flags or a port. Set ports in ~/.ssh/config.".into());
        }
        if !(self.folder.starts_with('/') || self.folder.starts_with("~/"))
            || self.folder.chars().any(char::is_control)
        {
            return Err(
                "Use an absolute remote folder or ~/folder, without control characters.".into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Upload {
    pub id: String,
    pub host_id: String,
    pub host_name: String,
    pub target: String,
    pub filename: String,
    pub path: String,
    pub time: String,
}
#[derive(Default, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    pub hosts: Vec<Host>,
    pub history: Vec<Upload>,
    pub selected: Option<String>,
    pub focused: bool,
}
impl State {
    pub fn toggle_focus(&mut self, id: &str) {
        self.focused = !(self.focused && self.selected.as_deref() == Some(id));
        self.selected = Some(id.to_owned());
    }
}
pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "sshawty")
        .expect("No user data directory")
        .data_local_dir()
        .to_owned()
}
pub fn load(dir: &Path) -> Result<State, String> {
    match fs::read(dir.join("state.json")) {
        Ok(bytes) => {
            let state: State = serde_json::from_slice(&bytes)
                .map_err(|e| format!("State could not be read; original preserved: {e}"))?;
            if state.version != 1 {
                return Err("Unsupported state version; original preserved.".into());
            }
            if state
                .history
                .iter()
                .any(|r| uuid::Uuid::parse_str(&r.id).is_err())
            {
                return Err("Invalid history identifier; original preserved.".into());
            }
            Ok(state)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State {
            version: 1,
            ..Default::default()
        }),
        Err(e) => Err(e.to_string()),
    }
}
pub fn save(dir: &Path, state: &State) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, state).map_err(|e| e.to_string())?;
    file.flush()
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(dir.join("state.json"))
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn thumbnail(dir: &Path, id: &str) -> PathBuf {
    dir.join("thumbnails").join(format!("{id}.png"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_and_roundtrip() {
        let mut h = Host {
            name: "Server".into(),
            target: "me@server".into(),
            ..Default::default()
        };
        assert!(h.validate().is_ok());
        h.target = "-oProxyCommand=bad".into();
        assert!(h.validate().is_err());
        h.target = "box".into();
        h.folder = "relative".into();
        assert!(h.validate().is_err());
        h.folder = "~/shots with 'quotes'".into();
        assert!(h.validate().is_ok());
        let dir = tempfile::tempdir().unwrap();
        let state = State {
            version: 1,
            hosts: vec![h],
            ..Default::default()
        };
        save(dir.path(), &state).unwrap();
        assert_eq!(load(dir.path()).unwrap().hosts[0].name, "Server");
        fs::write(dir.path().join("state.json"), "broken").unwrap();
        assert!(load(dir.path()).is_err());
    }
}

#[cfg(test)]
mod focus_tests {
    use super::*;
    #[test]
    fn focus_toggles_and_switches_hosts() {
        let mut state = State::default();
        state.toggle_focus("a");
        assert!(state.focused);
        assert_eq!(state.selected.as_deref(), Some("a"));
        state.toggle_focus("a");
        assert!(!state.focused);
        assert_eq!(state.selected.as_deref(), Some("a"));
        state.toggle_focus("a");
        state.toggle_focus("b");
        assert!(state.focused);
        assert_eq!(state.selected.as_deref(), Some("b"));
        state.toggle_focus("b");
        assert!(!state.focused);
    }
}
