use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::Path;
use std::rc::Rc;
use super::*;

pub struct PythonCompiler<'a> {
    pub out_path: PathBuf,
    pub backend: &'a PythonBackend,
}

impl<'a> Compiler<'a> for PythonCompiler<'a> {
    fn compile(&self) -> Result<()> {
        let files = self.populate_files()?;
        self.write_files(files)
    }
}

impl<'a> Collecting<'a> for FileSpec {
    type Processor = PythonCompiler<'a>;

    fn new() -> Self {
        FileSpec::new()
    }

    fn into_bytes(self, _: &Self::Processor) -> Result<Vec<u8>> {
        let mut out = String::new();
        self.format(&mut out)?;
        Ok(out.into_bytes())
    }
}

impl<'a> PackageProcessor<'a> for PythonCompiler<'a> {
    type Out = FileSpec;

    fn ext(&self) -> &str {
        EXT
    }

    fn env(&self) -> &Environment {
        &self.backend.env
    }

    fn out_path(&self) -> &Path {
        &self.out_path
    }

    fn processed_package(&self, package: &RpVersionedPackage) -> RpPackage {
        self.backend.package(package)
    }

    fn process_tuple(&self,
                     out: &mut Self::Out,
                     type_id: &RpTypeId,
                     pos: &RpPos,
                     body: Rc<RpTupleBody>)
                     -> Result<()> {
        self.backend.process_tuple(out, type_id, pos, body)
    }

    fn process_enum(&self,
                    out: &mut Self::Out,
                    type_id: &RpTypeId,
                    pos: &RpPos,
                    body: Rc<RpEnumBody>)
                    -> Result<()> {
        self.backend.process_enum(out, type_id, pos, body)
    }

    fn process_type(&self,
                    out: &mut Self::Out,
                    type_id: &RpTypeId,
                    pos: &RpPos,
                    body: Rc<RpTypeBody>)
                    -> Result<()> {
        self.backend.process_type(out, type_id, pos, body)
    }

    fn process_interface(&self,
                         out: &mut Self::Out,
                         type_id: &RpTypeId,
                         pos: &RpPos,
                         body: Rc<RpInterfaceBody>)
                         -> Result<()> {
        self.backend.process_interface(out, type_id, pos, body)
    }

    fn populate_files(&self) -> Result<BTreeMap<&RpVersionedPackage, FileSpec>> {
        let mut enums = Vec::new();

        let mut files = self.do_populate_files(|type_id, decl| {
                if let RpDecl::Enum(ref body) = *decl.as_ref() {
                    enums.push((type_id, body));
                }

                Ok(())
            })?;

        /// process static initialization of enums at bottom of file
        for (type_id, body) in enums {
            if let Some(ref mut file_spec) = files.get_mut(&type_id.package) {
                file_spec.push(self.backend.enum_variants(type_id, body)?);
            } else {
                return Err(format!("no such package: {}", &type_id.package).into());
            }
        }

        Ok(files)
    }

    fn resolve_full_path(&self, package: &RpPackage) -> Result<PathBuf> {
        let mut full_path = self.out_path().to_owned();
        let mut iter = package.parts.iter().peekable();

        while let Some(part) = iter.next() {
            full_path = full_path.join(part);

            if iter.peek().is_none() {
                continue;
            }

            if !full_path.is_dir() {
                debug!("+dir: {}", full_path.display());
                fs::create_dir_all(&full_path)?;
            }

            let init_path = full_path.join(INIT_PY);

            if !init_path.is_file() {
                debug!("+init: {}", init_path.display());
                File::create(init_path)?;
            }
        }

        // path to final file
        full_path.set_extension(self.ext());
        Ok(full_path)
    }
}