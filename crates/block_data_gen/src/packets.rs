use std::{io::Write, path::Path};
use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Packets(IndexMap<String, PacketRegistry>);

#[derive(Debug, Deserialize)]
struct PacketRegistry {
    pub clientbound: Option<IdMap>,
    pub serverbound: Option<IdMap>,
}

type IdMap = IndexMap<String, Entry>;

#[derive(Debug, Deserialize)]
struct Entry {
    pub protocol_id: u32,
}


impl Packets {
    pub fn generate(&self, folder: &Path) {
        std::fs::create_dir_all(folder).unwrap();
        let mut mod_file = std::fs::File::create(folder.join("mod.rs")).unwrap();


        for (category, registry) in self.0.iter() {
            writeln!(&mut mod_file, "pub mod {};", category).unwrap();

            let folder: &Path = &folder.join(category);
            std::fs::create_dir_all(folder).unwrap();
            let mut mod_file = std::fs::File::create(folder.join("mod.rs")).unwrap();

            for (bound, ty, id_map) in [("clientbound", "u32", &registry.clientbound), ("serverbound", "i32", &registry.serverbound)] {
                let Some(id_map) = id_map else {
                    continue;
                };

                writeln!(&mut mod_file, "pub mod {};", bound).unwrap();

                let mut file = std::fs::File::create(folder.join(format!("{}.rs", bound))).unwrap();

                let mut ids = id_map.iter().collect::<Vec<_>>();
                // ids.sort_by_key(|(_, id)| id.protocol_id);

                for (name, id) in ids {
                    let name = name.strip_prefix("minecraft:").unwrap();
                    let name = name.to_uppercase();
                    let id = id.protocol_id;

                    writeln!(file, "pub const {}: {} = 0x{:02x};", name, ty, id).unwrap();
                }
            }
        }
    }
}