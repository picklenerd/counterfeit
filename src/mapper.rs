use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::task::{Context, Poll};
use std::sync::Arc;

use anyhow::Result;
use futures::future;
use hyper::{Body, Request, Response, StatusCode};
use hyper::service::Service;
use hyper::header::{self, HeaderValue};

pub mod dir_picker;
pub mod file_picker;
pub mod mutation;

pub use crate::mapper::dir_picker::{DirPicker, StandardDirPicker};
pub use crate::mapper::file_picker::{FilePicker, StandardFilePicker};
pub use crate::mapper::mutation::ResponseMutation;

use crate::{CounterfeitRunConfig, MultiFileIndexMap};

pub struct FileMapperService<D, F>
where
    D: DirPicker,
    F: FilePicker,
{
    dir_picker: D,
    file_picker: F,
    mutations: Vec<Box<dyn ResponseMutation>>,
    config: CounterfeitRunConfig,
}

impl<D, F> FileMapperService<D, F>
where
    D: DirPicker,
    F: FilePicker,
{
    pub fn new(
        dir_picker: D,
        file_picker: F,
        mutations: Vec<Box<dyn ResponseMutation>>,
        config: CounterfeitRunConfig,
    ) -> Self {
        Self {
            dir_picker,
            file_picker,
            mutations,
            config,
        }
    }

    pub fn add_mutation(&mut self, mutation: impl ResponseMutation + 'static) {
        self.mutations.push(Box::new(mutation));
    }
}

impl FileMapperService<StandardDirPicker, StandardFilePicker> {
    pub fn standard(config: CounterfeitRunConfig, index_map: MultiFileIndexMap) -> Self {
        Self {
            dir_picker: StandardDirPicker::new(config.clone()),
            file_picker: StandardFilePicker::new(config.create_missing, index_map),
            mutations: Vec::new(),
            config,
        }
    }
}

impl<D, F> Service<Request<Body>> for FileMapperService<D, F>
where
    D: DirPicker,
    F: FilePicker,
{
    type Response = Response<Body>;
    type Error = anyhow::Error;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        if !self.config.silent {
            println!("Request: {} -> {}", request.method(), request.uri().path());
        }

        match self.dir_picker.pick_directory(&request) {
            Ok(directory) => {
                let file = self.file_picker.pick_file(&directory, &request);
                let mut output = MapperOutput::new(request, file);
        
                for mutation in self.mutations.iter() {
                    if let Err(e) = mutation.apply_mutation(&mut output) {
                        return future::err(e.into());
                    }
                }
        
                if !self.config.silent {
                    println!("Response: {} -> {}", output.response.status(), output);
                }
        
                future::ok(output.into())
            },
            Err(e) => future::err(e.into()),
        }
    }
}

pub struct MakeFileMapperService {
    config: CounterfeitRunConfig,
    index_map: MultiFileIndexMap,
}

impl MakeFileMapperService {
    pub fn new(config: CounterfeitRunConfig, index_map: MultiFileIndexMap) -> Self {
        Self {
            config,
            index_map,
        }
    }
}

impl<T> Service<T> for MakeFileMapperService {
    type Response = FileMapperService<StandardDirPicker, StandardFilePicker>;
    type Error = anyhow::Error;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Ok(()).into()
    }

    fn call(&mut self, _: T) -> Self::Future {
        future::ok(FileMapperService::standard(self.config.clone(), Arc::clone(&self.index_map)))
    }
}

pub type MapperResult = Result<PathBuf, io::Error>;

#[derive(Debug)]
pub struct MapperOutput {
    request: Request<Body>,
    response: Response<Body>,
    result: MapperResult,
}

impl MapperOutput {
    pub fn new(request: Request<Body>, result: MapperResult) -> Self {
        let response = match &result {
            Ok(path) => Self::response_from_file(path),
            Err(e) => Self::response_from_error(e),
        };

        Self {
            request,
            response,
            result,
        }
    }

    fn response_from_file<P: AsRef<Path>>(file_path: P) -> Response<Body> {
        match fs::read_to_string(&file_path) {
            Ok(path) => {
                let mut response = Response::new(Body::from(path));
                *response.status_mut() = StatusCode::OK;
                set_default_headers(&mut response);
                response
            },
            Err(e) => Self::response_from_error(&e),
        }
    }

    fn response_from_error(error: &io::Error) -> Response<Body> {
        let mut response = Response::new(Body::from(format!("{}", error)));
        *response.status_mut() = match error.kind() {
            io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        set_default_headers(&mut response);
        response
    }

    pub fn request(&self) -> &Request<Body> {
        &self.request
    }

    pub fn response(&self) -> &Response<Body> {
        &self.response
    }

    pub fn response_mut(&mut self) -> &mut Response<Body> {
        &mut self.response
    }

    pub fn result(&self) -> &MapperResult {
        &self.result
    }

    pub fn result_mut(&mut self) -> &mut MapperResult {
        &mut self.result
    }
}

impl std::fmt::Display for MapperOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.result {
            Ok(path) => write!(f, "{}", path.display()),
            Err(e) => write!(f, "{}", e),
        }
    }
}

impl From<MapperOutput> for Response<Body> {
    fn from(mapper_output: MapperOutput) -> Response<Body> {
        mapper_output.response
    }
}

fn set_default_headers(response: &mut Response<Body>) {
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );

    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("*"),
    );

    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("*"),
    );
}
