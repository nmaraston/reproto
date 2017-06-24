use linked_hash_map::{self, LinkedHashMap};
use parser;
use reproto_parser::ast::IntoModel;
use reproto_repository::Resolver;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use super::*;

pub type InitFields = HashMap<String, RpLoc<RpFieldInit>>;

pub struct Environment {
    package_prefix: Option<RpPackage>,
    resolver: Box<Resolver>,
    visited: HashSet<RpVersionedPackage>,
    pub types: LinkedHashMap<RpTypeId, RpLoc<RpRegistered>>,
    pub decls: LinkedHashMap<RpTypeId, Rc<RpLoc<RpDecl>>>,
    pub used: LinkedHashMap<(RpVersionedPackage, String), RpVersionedPackage>,
}

impl Environment {
    pub fn new(package_prefix: Option<RpPackage>, resolver: Box<Resolver>) -> Environment {
        Environment {
            package_prefix: package_prefix,
            resolver: resolver,
            visited: HashSet::new(),
            types: LinkedHashMap::new(),
            decls: LinkedHashMap::new(),
            used: LinkedHashMap::new(),
        }
    }

    fn into_registered_type(&self,
                            package: &RpVersionedPackage,
                            decl: Rc<RpLoc<RpDecl>>)
                            -> Result<Vec<(RpTypeId, RpLoc<RpRegistered>)>> {
        let mut out = Vec::new();

        // apply package prefix, if needed
        let package = self.package_prefix
            .as_ref()
            .map(|prefix| prefix.join_versioned(package))
            .unwrap_or_else(|| package.clone());

        match **decl {
            RpDecl::Type(ref ty) => {
                let type_id = package.into_type_id(RpName::with_parts(vec![ty.name.clone()]));
                let token = RpLoc::new(RpRegistered::Type(ty.clone()), decl.pos().clone());
                out.push((type_id, token));
            }
            RpDecl::Interface(ref interface) => {
                let current = vec![interface.name.clone()];
                let type_id = RpTypeId::new(package.clone(), RpName::with_parts(current.clone()));
                let token = RpLoc::new(RpRegistered::Interface(interface.clone()),
                                       decl.pos().clone());

                for (name, sub_type) in &interface.sub_types {
                    let sub_type = RpRegistered::SubType {
                        parent: interface.clone(),
                        sub_type: sub_type.as_ref().clone(),
                    };

                    let token = RpLoc::new(sub_type, decl.pos().clone());

                    let mut current = current.clone();
                    current.push(name.to_owned());
                    out.push((type_id.with_name(RpName::with_parts(current)), token));
                }

                out.push((type_id, token));
            }
            RpDecl::Enum(ref en) => {
                let current = vec![en.name.clone()];
                let type_id = RpTypeId::new(package.clone(), RpName::with_parts(current.clone()));
                let token = RpLoc::new(RpRegistered::Enum(en.clone()), decl.pos().clone());

                for variant in &en.variants {
                    let enum_constant = RpRegistered::EnumConstant {
                        parent: en.clone(),
                        variant: variant.as_ref().clone(),
                    };
                    let token = RpLoc::new(enum_constant, decl.pos().clone());

                    let mut current = current.clone();
                    current.push((*variant.name).to_owned());
                    out.push((type_id.with_name(RpName::with_parts(current)), token));
                }

                out.push((type_id, token));
            }
            RpDecl::Tuple(ref tuple) => {
                let type_id = RpTypeId::new(package.clone(),
                                            RpName::with_parts(vec![tuple.name.clone()]));
                let token = RpLoc::new(RpRegistered::Tuple(tuple.clone()), decl.pos().clone());
                out.push((type_id, token));
            }
            RpDecl::Service(ref service) => {
                let type_id = RpTypeId::new(package.clone(),
                                            RpName::with_parts(vec![service.name.clone()]));
                let token = RpLoc::new(RpRegistered::Service(service.clone()), decl.pos().clone());
                out.push((type_id, token));
            }
        }

        Ok(out)
    }

    fn register_alias(&mut self,
                      source_package: &RpVersionedPackage,
                      use_decl: RpLoc<RpUseDecl>,
                      use_package: &RpVersionedPackage)
                      -> Result<()> {
        if let Some(used) = use_decl.package.parts.iter().last() {
            let alias = if let Some(ref next) = use_decl.alias {
                next
            } else {
                used
            };

            let key = (source_package.clone(), alias.clone());

            debug!("add alias {} ({})", alias, source_package);

            match self.used.entry(key) {
                linked_hash_map::Entry::Vacant(entry) => {
                    entry.insert(use_package.clone());
                }
                linked_hash_map::Entry::Occupied(_) => {
                    return Err(format!("alias {} already in use", alias).into())
                }
            };
        }

        Ok(())
    }

    pub fn is_assignable_from(&self,
                              package: &RpVersionedPackage,
                              target: &RpType,
                              source: &RpType)
                              -> Result<bool> {
        match (target, source) {
            (&RpType::Double, &RpType::Double) => Ok(true),
            (&RpType::Float, &RpType::Float) => Ok(true),
            (&RpType::Signed { size: Some(ref target) },
             &RpType::Signed { size: Some(ref source) }) => Ok(target <= source),
            // unknown size matches known
            (&RpType::Signed { size: _ }, &RpType::Signed { size: None }) => Ok(true),
            (&RpType::Unsigned { size: Some(ref target) },
             &RpType::Unsigned { size: Some(ref source) }) => Ok(target <= source),
            // unknown size matches known
            (&RpType::Unsigned { size: _ }, &RpType::Unsigned { size: None }) => Ok(true),
            (&RpType::Boolean, &RpType::Boolean) => return Ok(true),
            (&RpType::String, &RpType::String) => return Ok(true),
            (&RpType::Bytes, &RpType::Bytes) => return Ok(true),
            // everything assignable to any type
            (&RpType::Any, _) => Ok(true),
            (&RpType::Name { name: ref target }, &RpType::Name { name: ref source }) => {
                let (_, target) = self.lookup(package, target)?;
                let (_, source) = self.lookup(package, source)?;
                return Ok(target.is_assignable_from(source));
            }
            // arrays match if inner type matches
            (&RpType::Array { inner: ref target }, &RpType::Array { inner: ref source }) => {
                return self.is_assignable_from(package, target, source);
            }
            (&RpType::Map { key: ref target_key, value: ref target_value },
             &RpType::Map { key: ref source_key, value: ref source_value }) => {
                let key_assignable = self.is_assignable_from(package, target_key, source_key)?;
                let value_assignable =
                    self.is_assignable_from(package, target_value, source_value)?;

                return Ok(key_assignable && value_assignable);
            }
            _ => Ok(false),
        }
    }

    pub fn constant<'a>(&'a self,
                        pos: &RpPos,
                        package: &'a RpVersionedPackage,
                        constant: &RpName,
                        target: &RpName)
                        -> Result<&'a RpRegistered> {
        let (_, reg_constant) = self.lookup(package, constant)
            .map_err(|e| Error::pos(e.description().to_owned(), pos.into()))?;

        let (_, reg_target) = self.lookup(package, target)
            .map_err(|e| Error::pos(e.description().to_owned(), pos.into()))?;

        if !reg_target.is_assignable_from(reg_constant) {
            return Err(Error::pos(format!("expected instance of `{}` but found `{}`",
                                          reg_target.display(),
                                          reg_constant.display()),
                                  pos.into()));
        }

        Ok(reg_constant)
    }

    /// Convert instance arguments to the known registered type of the instance, and a map
    /// containing the arguments being instantiated.
    pub fn instance<'a>(&'a self,
                        pos: &RpPos,
                        package: &'a RpVersionedPackage,
                        instance: &RpInstance,
                        target: &RpName)
                        -> Result<(&'a RpRegistered, InitFields)> {
        let (_, reg_instance) = self.lookup(package, &instance.name)
            .map_err(|e| Error::pos(e.description().to_owned(), pos.into()))?;

        let (_, reg_target) = self.lookup(package, target)
            .map_err(|e| Error::pos(e.description().to_owned(), pos.into()))?;

        if !reg_target.is_assignable_from(reg_instance) {
            return Err(Error::pos(format!("expected instance of `{}` but found `{}`",
                                          reg_target.display(),
                                          reg_instance.display()),
                                  pos.into()));
        }

        let required_fields = match *reg_instance {
            RpRegistered::Type(ref ty) => ty.fields(),
            RpRegistered::SubType { ref parent, ref sub_type } => {
                Box::new(parent.fields().chain(sub_type.fields()))
            }
            RpRegistered::Tuple(ref tuple) => tuple.fields(),
            _ => return Err(Error::pos("expected instantiable type".into(), pos.into())),
        };

        // pick required fields.
        let required_fields = required_fields.filter(|f| f.modifier == RpModifier::Required);

        let mut known: HashMap<String, RpLoc<RpFieldInit>> = HashMap::new();

        // check that all required fields are set.
        let mut required: BTreeMap<String, RpLoc<RpField>> = required_fields.map(Clone::clone)
            .map(|f| (f.name().to_owned(), f))
            .collect();

        for init in &*instance.arguments {
            if let Some(ref field) = reg_instance.field_by_ident(&init.name)? {
                // TODO: map out init position, and check that required variables are set.
                known.insert(field.ident().to_owned(), init.clone());
                required.remove(field.name());
            } else {
                return Err(Error::pos("no such field".to_owned(), init.pos().into()));
            }
        }

        if !required.is_empty() {
            let required: Vec<(String, RpLoc<RpField>)> = required.into_iter()
                .collect();

            let names: Vec<String> =
                required.iter().map(|&(ref name, _)| name.to_owned()).collect();

            let positions: Vec<ErrorPos> =
                required.iter().map(|&(_, ref t)| t.pos().into()).collect();

            return Err(ErrorKind::MissingRequired(names,
                                                  instance.arguments.pos().into(),
                                                  positions)
                .into());
        }

        Ok((reg_instance, known))
    }

    /// Lookup the package declaration a used alias refers to.
    fn lookup_used(&self, package: &RpVersionedPackage, used: &str) -> Result<&RpVersionedPackage> {
        // resolve alias
        self.used
            .get(&(package.clone(), used.to_owned()))
            .ok_or_else(|| format!("not import for alias ({})", used).into())
    }

    /// Lookup the declaration matching the custom type.
    pub fn lookup<'a>(&'a self,
                      package: &'a RpVersionedPackage,
                      lookup_name: &RpName)
                      -> Result<(&'a RpVersionedPackage, &'a RpRegistered)> {
        let (package, name) = if let Some(ref prefix) = lookup_name.prefix {
            (self.lookup_used(package, prefix)?, lookup_name.without_prefix())
        } else {
            (package, lookup_name.clone())
        };

        let types_key = RpTypeId::new(package.clone(), name);

        if let Some(ty) = self.types.get(&types_key) {
            return Ok((package, ty));
        }

        return Err(format!("no such type: {}", lookup_name).into());
    }

    pub fn load_file(&mut self,
                     path: &Path,
                     version: Option<Version>,
                     package: Option<RpPackage>)
                     -> Result<Option<(RpVersionedPackage, RpFile)>> {
        let content = parser::read_file(&path)?;
        let file = parser::parse_file(&path, content.as_str())?.into_model()?;
        let package = RpVersionedPackage::new(package, version);
        Ok(Some((package, file)))
    }

    pub fn process_uses(&mut self,
                        package: &RpVersionedPackage,
                        uses: Vec<RpLoc<RpUseDecl>>)
                        -> Result<()> {
        for use_decl in uses {
            let version_req = use_decl.version_req.as_ref().map(AsRef::as_ref).map(Clone::clone);
            let required = RpRequiredPackage::new(use_decl.package.as_ref().clone(), version_req);

            let use_package = self.import(&required)?;

            if let Some(use_package) = use_package {
                self.register_alias(package, use_decl, &use_package)?;
                continue;
            }

            let error = "no matching package found".to_owned();
            return Err(ErrorKind::Pos(error, use_decl.pos().into()).into());
        }

        Ok(())
    }

    pub fn process_decls(package: &RpVersionedPackage,
                         input: Vec<RpLoc<RpDecl>>)
                         -> Result<LinkedHashMap<RpTypeId, Rc<RpLoc<RpDecl>>>> {
        let mut decls = LinkedHashMap::new();

        for decl in input {
            let custom = RpName::with_parts(vec![decl.name().to_owned()]);
            let key = package.into_type_id(custom);

            match decls.entry(key) {
                linked_hash_map::Entry::Vacant(entry) => {
                    entry.insert(Rc::new(decl));
                }
                linked_hash_map::Entry::Occupied(entry) => {
                    entry.into_mut().merge(Rc::new(decl))?;
                }
            }
        }

        Ok(decls)
    }

    pub fn process_types(&mut self,
                         package: &RpVersionedPackage,
                         decls: &LinkedHashMap<RpTypeId, Rc<RpLoc<RpDecl>>>)
                         -> Result<LinkedHashMap<RpTypeId, RpLoc<RpRegistered>>> {
        let mut types = LinkedHashMap::new();

        for (_, decl) in decls {
            let registered_types = self.into_registered_type(package, decl.clone())?;

            for (key, t) in registered_types.into_iter() {
                if let Some(_) = types.insert(key.clone(), t) {
                    return Err(ErrorKind::RegisteredTypeConflict(key.clone()).into());
                }
            }
        }

        Ok(types)
    }

    pub fn process_file(&mut self, package: &RpVersionedPackage, file: RpFile) -> Result<()> {
        self.process_uses(&package, file.uses)?;
        let decls = Self::process_decls(&package, file.decls)?;
        let types = self.process_types(&package, &decls)?;
        self.decls.extend(decls);
        self.types.extend(types);
        Ok(())
    }

    pub fn find_visited_by_required(&self,
                                    required: &RpRequiredPackage)
                                    -> Option<RpVersionedPackage> {
        for visited in &self.visited {
            if let Some(ref visited_package) = visited.package {
                if *visited_package == required.package {
                    if let Some(ref version_req) = required.version_req {
                        if let Some(ref actual_version) = visited.version {
                            if version_req.matches(actual_version) {
                                return Some(visited.clone());
                            }
                        }
                    } else {
                        return Some(visited.clone());
                    }
                }
            }
        }

        None
    }

    pub fn import_file(&mut self, path: &Path) -> Result<Option<RpVersionedPackage>> {
        if let Some((package, file)) = self.load_file(path, None, None)? {
            if !self.visited.contains(&package) {
                self.process_file(&package, file)?;
                self.visited.insert(package.clone());
            }

            return Ok(Some(package));
        }

        Ok(None)
    }

    pub fn import(&mut self, required: &RpRequiredPackage) -> Result<Option<RpVersionedPackage>> {
        debug!("import: {}", required);

        if let Some(existing) = self.find_visited_by_required(required) {
            debug!("already loaded: {} ({})", existing, required);
            return Ok(Some(existing));
        }

        let files = self.resolver.resolve(required)?;

        let mut candidates: BTreeMap<RpVersionedPackage, Vec<_>> = BTreeMap::new();

        for (version, path) in files {
            debug!("loading: {}", path.display());

            let loaded = self.load_file(&path, version, Some(required.package.clone()))?;

            if let Some((package, file)) = loaded {
                candidates.entry(package).or_insert_with(Vec::new).push((path, file));
            }
        }

        if let Some((versioned, files)) = candidates.into_iter().nth(0) {
            debug!("found: {} ({})", versioned, required);

            for (path, file) in files.into_iter() {
                debug!("in: {}", path.display());
                self.process_file(&versioned, file)?;
            }

            self.visited.insert(versioned.clone());
            return Ok(Some(versioned));
        }

        Ok(None)
    }

    pub fn verify(&mut self) -> Result<()> {
        for (_, ref ty) in &self.decls {
            match ****ty {
                RpDecl::Type(ref ty) => {
                    ty.verify()?;
                }
                _ => {}
            }
        }

        Ok(())
    }
}
