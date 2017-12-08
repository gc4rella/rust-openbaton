extern crate serde;

#[derive(Debug, Deserialize, Serialize)]
pub struct BaseVimInstance {
    pub name: String,
    pub _type: String,
    pub auth_url: String,
    pub active: bool,
    pub location: Location,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Location {
    pub name: String,
    pub latitude: String,
    pub longitude: String,
}


impl BaseVimInstance {
    pub fn new() -> BaseVimInstance {
        BaseVimInstance {
            name: String::new(),
            _type: String::new(),
            auth_url: String::new(),
            active: true,
            location: Location::new(),
        }
    }
}

impl Location {
    pub fn new() -> Location {
        Location {
            name: String::new(),
            latitude: String::new(),
            longitude: String::new(),
        }
    }
}
