use parser::ast;
use parser;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use super::errors::*;
use super::into_model::IntoModel;
use super::models::*;

const EXT: &str = "reproto";

pub type TypeId = (Package, String);

pub struct Environment {
    paths: Vec<PathBuf>,
    visited: HashSet<Package>,
    pub types: BTreeMap<TypeId, Token<Decl>>,
    pub used: BTreeMap<(Package, String), Package>,
}

impl Environment {
    pub fn new(paths: Vec<PathBuf>) -> Environment {
        Environment {
            paths: paths,
            visited: HashSet::new(),
            types: BTreeMap::new(),
            used: BTreeMap::new(),
        }
    }

    fn register_type(&mut self, package: &Package, decl: Token<Decl>) -> Result<()> {
        let key = (package.clone(), decl.name().to_owned());

        match self.types.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(decl);
            }
            Entry::Occupied(entry) => {
                let target = entry.into_mut();
                let target_pos = target.pos.clone();

                match target.inner {
                    Decl::Type(ref mut body) => {
                        if let Decl::Type(other) = decl.inner {
                            body.merge(other)?;
                        } else {
                            return Err(Error::decl_merge(format!("Cannot merge {}",
                                                                 decl.display()),
                                                         decl.pos,
                                                         target_pos));
                        }
                    }
                    Decl::Enum(ref mut body) => {
                        if let Decl::Enum(other) = decl.inner {
                            body.merge(other)?;
                        } else {
                            return Err(Error::decl_merge(format!("Cannot merge {}",
                                                                 decl.display()),
                                                         decl.pos,
                                                         target_pos));
                        }
                    }
                    _ => return Err("not yet supported".into()),
                }
            }
        };

        Ok(())
    }

    fn register_alias(&mut self, package: &Package, use_decl: &ast::UseDecl) -> Result<()> {
        if let Some(used) = use_decl.package.parts.iter().last() {
            let alias = if let Some(ref next) = use_decl.alias {
                next
            } else {
                used
            };

            let key = (package.clone(), alias.clone());

            match self.used.entry(key) {
                Entry::Vacant(entry) => entry.insert(use_decl.package.inner.clone()),
                Entry::Occupied(_) => return Err(format!("alias {} already in used", alias).into()),
            };
        }

        Ok(())
    }

    /// Lookup the package declaration a used alias refers to.
    pub fn lookup_used(&self, pos: &Pos, package: &Package, used: &str) -> Result<&Package> {
        // resolve alias
        let package =
            self.used
                .get(&(package.clone(), used.to_owned()))
                .ok_or_else(|| {
                    Error::pos(format!("Missing import alias for ({})", used), pos.clone())
                })?;

        // check that type actually exists?
        let key = (package.clone(), used.to_owned());
        let _ = self.types.get(&key);

        Ok(package)
    }

    pub fn import_file(&mut self, path: &Path, package: Option<&Package>) -> Result<()> {
        debug!("in: {}", path.display());

        // TODO: fix this
        let file = parser::parse_file(&path)?;

        if let Some(package) = package {
            if *file.package != *package {
                return Err(format!("Expected package ({}) in file {}, but was ({})",
                                   package,
                                   path.display(),
                                   *file.package)
                    .into());
            }
        }

        for use_decl in &file.uses {
            self.register_alias(&file.package, use_decl)?;
            self.import(&use_decl.package)?;
        }

        for decl in file.decls {
            let decl = decl.into_model(path)?;
            self.register_type(&file.package, decl)?;
        }

        Ok(())
    }

    pub fn import(&mut self, package: &Package) -> Result<()> {
        if self.visited.contains(package) {
            return Ok(());
        }

        self.visited.insert(package.clone());

        let mut files: Vec<PathBuf> = Vec::new();

        let candidates: Vec<PathBuf> = self.paths
            .iter()
            .map(|p| {
                let mut path = p.clone();

                for part in &package.parts {
                    path.push(part);
                }

                path.set_extension(EXT);
                path
            })
            .collect();

        for path in &candidates {
            if !path.is_file() {
                continue;
            }

            files.push(path.clone());
        }

        if files.len() == 0 {
            let candidates_format: Vec<String> = candidates.iter()
                .map(|c| format!("{}", c.display()))
                .collect();

            let candidates_format = candidates_format.join(", ");

            return Err(format!("No files matching package ({}), expected one of: {}",
                               *package,
                               candidates_format)
                .into());
        }

        for path in files {
            self.import_file(&path, Some(package))?;
        }

        Ok(())
    }
}
