use std::collections::HashMap;

pub struct LibraryLoader {
    known: HashMap<&'static str, &'static str>,
    loaded: HashMap<String, ()>,
}

impl LibraryLoader {
    pub fn new() -> Self {
        let mut known = HashMap::new();
        known.insert("အချိန်", "Time / sleep capabilities (စောင့်ပါ)");
        LibraryLoader {
            known,
            loaded: HashMap::new(),
        }
    }

    pub fn load(&mut self, name: &str) -> Result<(), String> {
        if self.known.contains_key(name) {
            self.loaded.insert(name.to_string(), ());
            Ok(())
        } else {
            Err(format!("library '{}' not found", name))
        }
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
