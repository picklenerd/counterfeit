use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{DefaultRequest, FilePicker, MultiFileIndexMap};
use crate::Error;

pub struct DefaultFilePicker {
    create_missing: bool,
    multifile_indices: MultiFileIndexMap,
}


impl DefaultFilePicker {
    pub fn new(create_missing: bool, index_map: MultiFileIndexMap) -> Self {
        Self {
            create_missing,
            multifile_indices: index_map,
        }
    }
}

impl FilePicker<DefaultRequest> for DefaultFilePicker {
    fn pick_file(&self, directory: &Path, request: &DefaultRequest) -> Result<PathBuf, Error> {
        let available_files = fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| p.is_file())
            .filter(|p| file_matches(p, &request.method))
            .collect::<Vec<PathBuf>>();

        if available_files.is_empty() {
            if self.create_missing {
                let file_name = format!("{}.json", request.method.to_lowercase());
                
                let mut path = PathBuf::new();
                path.push(directory);
                path.push(file_name);
                
                fs::File::create(&path)?;
                
                Ok(path)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "No files available",
                ).into())
            }
        } else {
            let mut indices = self.multifile_indices.lock().unwrap();
            let index = indices.entry(PathBuf::from(directory)).or_insert_with(|| 0);
            if *index >= available_files.len() {
                *index = 0;
            }

            match available_files.into_iter().nth(*index) {
                Some(file) => {
                    *index += 1;
                    Ok(file)
                }
                None => Err(io::Error::new(io::ErrorKind::Other, "Could not read file").into()),
            }
        }
    }
}

fn file_matches(file_path: &PathBuf, method: &str) -> bool {
    let method_str = method.to_lowercase();

    match file_path.file_stem().and_then(|stem| stem.to_str()) {
        Some(stem) => {
            stem == method_str || stem.to_lowercase().starts_with(&format!("{}_", method_str))
        }
        None => false,
    }
}
