// Sample Rust file for benchmarking
use std::collections::HashMap;

pub fn main() {
    let mut map = HashMap::new();
    map.insert("key", "value");
    println!("{:?}", map);
}

pub struct SampleStruct {
    pub field: String,
}

impl SampleStruct {
    pub fn new() -> Self {
        Self {
            field: "default".to_string(),
        }
    }
}
