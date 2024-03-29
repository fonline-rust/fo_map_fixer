use fo_map_format::{verbose_read_file, MapObjectType, MapParserSettings};
use fo_proto_format::ProtoItem;
use nom_prelude::nom_err_to_string;
use rayon::prelude::*;
use std::{
    collections::btree_map::BTreeMap,
    ffi::OsStr,
    io::Write as _,
    path::{Path, PathBuf},
    time::Instant,
};

fn items() -> BTreeMap<u16, ProtoItem> {
    let mut dir = dir("PROTO_PATH", "proto_path.cfg")
        .unwrap_or("../../proto".into())
        .canonicalize()
        .unwrap();
    dir.push("items");
    dir.push("items.lst");
    fo_proto_format::build_btree(dir)
}

fn item_type_to_map_type(item_type: u8) -> MapObjectType {
    match item_type {
        10 | 11 | 12 => MapObjectType::MAP_OBJECT_SCENERY, // ITEM_TYPE_GRID, ITEM_TYPE_GENERIC, ITEM_TYPE_WALL => MAP_OBJECT_SCENERY
        _ => MapObjectType::MAP_OBJECT_ITEM,               // evything else => MAP_OBJECT_ITEM
    }
}

fn main() {
    let items = items();

    let dir = dir("MAPS_PATH", "maps_path.cfg")
        .unwrap_or("../../maps".into())
        .canonicalize()
        .unwrap();

    let maps: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|r| r.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension() == Some("fomap".as_ref()))
        .collect();
    println!("Found {} maps.", maps.len());
    let instant = Instant::now();

    let invalids: Vec<Vec<u8>> = maps
        .par_iter()
        .flat_map(|file| {
            println!("Parsing {:?}", file);
            let mut invalid = None::<Vec<u8>>;

            let settings = MapParserSettings { allow_any: true };

            let changes = verbose_read_file(
                &file,
                |text, res| {
                    let (rest, map) = nom_err_to_string(text, res).expect("Can't parse map file");
                    if !rest.is_empty() {
                        dbg!(rest);
                        panic!("Rest is not empty!");
                    }

                    let text_bytes = text.as_bytes().as_ptr();
                    map.objects
                        .0
                        .iter()
                        .filter(|obj| obj.kind.is_any())
                        .for_each(|obj| {
                            if invalid.is_none() {
                                invalid = Some(format!("File: {:?}\n", file).into());
                            }
                            let writer = invalid.as_mut().unwrap();
                            serde_json::to_writer(&mut *writer, obj).unwrap();
                            writer.push(b'\n');
                        });
                    let changes: Vec<_> = map
                        .objects
                        .0
                        .iter()
                        .rev()
                        .filter(|obj| {
                            obj.kind.map_object_type() != MapObjectType::MAP_OBJECT_CRITTER
                        })
                        .filter_map(|obj| {
                            items
                                .get(&obj.proto_id)
                                .map(|proto| item_type_to_map_type(proto.Type))
                                .filter(|proto_map_type| {
                                    *proto_map_type != obj.kind.map_object_type()
                                })
                                .map(|proto_map_type| {
                                    let bytes = obj.ty_str.as_bytes();
                                    let offset =
                                        u64::wrapping_sub(bytes.as_ptr() as _, text_bytes as _);
                                    (offset, bytes.len(), proto_map_type as u8)
                                })
                        })
                        .collect();
                    changes
                },
                settings,
            )
            .expect("Can't read map file");

            if let Some(invalid) = invalid.as_mut() {
                invalid.push(b'\n');
            }

            if !changes.is_empty() {
                println!("Writing {} changes to {:?}", changes.len(), file);
                //std::fs::copy(&file, file.with_extension("fomap.backup")).expect("Backup copy");

                let mut file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .truncate(false)
                    .open(file)
                    .expect("Open map file to write changes");

                for (offset, len, val) in changes {
                    assert_eq!(len, 1);
                    assert!(val <= 9);
                    let buf = [b'0' as u8 + val];
                    use std::io::{Seek, SeekFrom, Write};
                    file.seek(SeekFrom::Start(offset)).expect("Seek file");
                    file.write(&buf).expect("Write new type value to file");
                }
            }
            invalid
        })
        .collect();
    let elapsed = instant.elapsed();
    println!(
        "Checked {} maps in {:.2} seconds.",
        maps.len(),
        elapsed.as_secs_f32()
    );
    let mut invalids_file = std::fs::File::create("invalid_objects.txt").unwrap();
    if !invalids.is_empty() {
        for invalid in &invalids {
            invalids_file.write_all(&invalid).unwrap();
        }
        println!(
            "Objects with invalid fields found in {} maps. Check them in invalid_objects.txt",
            invalids.len()
        );
    }
}

fn dir<P1: AsRef<OsStr>, P2: AsRef<Path>>(env: P1, file: P2) -> Option<PathBuf> {
    let env = std::env::var_os(env);
    if let Some(path) = env.and_then(|env| Path::new(&env).canonicalize().ok()) {
        Some(path)
    } else if let Ok(path) =
        std::fs::read_to_string(file).and_then(|env| Path::new(env.trim()).canonicalize())
    {
        Some(path)
    } else {
        None
    }
}
