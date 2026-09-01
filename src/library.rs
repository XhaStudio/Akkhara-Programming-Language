use std::collections::HashMap;

pub struct LibraryLoader {
    loaded: HashMap<String, ()>,
}
 
impl LibraryLoader {
    pub fn new() -> Self {
        LibraryLoader {
            loaded: HashMap::new(),
        }
    }

    pub fn load(&mut self, name: &str) -> Result<(), String> {
        self.loaded.insert(name.to_string(), ());
        Ok(())
    }

    pub fn is_loaded(&self, name: &str) -> bool {
        self.loaded.contains_key(name)
    }
}

impl Default for LibraryLoader {
    fn default() -> Self {
        Self::new()
    }
}
