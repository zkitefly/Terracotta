use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum ProfileKind {
    HOST, LOCAL, GUEST
}

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    machine_id: String,
    name: String,
    kind: ProfileKind,
}

pub struct ProfileSnapshot {
    pub machine_id: String,
    pub name: String,
    pub kind: ProfileKind,
}

impl Profile {
    pub fn new(machine_id: String, name: String, kind: ProfileKind) -> Profile {
        Profile {machine_id, name, kind}
    }

    pub fn get_machine_id(&self) -> &str {
        &self.machine_id
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_kind(&self) -> &ProfileKind {
        &self.kind
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn as_snapshot(&self) -> ProfileSnapshot {
        ProfileSnapshot { machine_id: self.machine_id.clone(), name: self.name.clone(), kind: self.kind }
    }
}
