use environment::Environment;
use options::Options;
use parser::ast;
use std::fs::File;
use std::path::PathBuf;
use std::fs;
use std::io::Write;
use naming::{self, FromNaming};
use std::collections::HashMap;
use std::collections::hash_map::Entry;

#[macro_use]
use codegen::python::*;

use errors::*;

const INIT_PY: &str = "__init__.py";
const EXT: &str = "py";

pub trait Listeners {
    fn class_added(&self,
                   processor: &Processor,
                   package: &ast::Package,
                   fields: &Vec<Field>,
                   class: &mut ClassSpec)
                   -> Result<()>;

    fn tuple_added(&self,
                   processor: &Processor,
                   package: &ast::Package,
                   fields: &Vec<Field>,
                   class: &mut ClassSpec)
                   -> Result<()>;

    fn interface_added(&self,
                       processor: &Processor,
                       package: &ast::Package,
                       interface: &ast::InterfaceDecl,
                       interface_spec: &mut ClassSpec)
                       -> Result<()>;
}

#[derive(Clone)]
pub struct Field {
    pub modifier: ast::Modifier,
    pub ty: ast::Type,
    pub name: String,
    pub ident: String,
}

impl Field {
    pub fn new(modifier: ast::Modifier, ty: ast::Type, name: String, ident: String) -> Field {
        Field {
            modifier: modifier,
            ty: ty,
            name: name,
            ident: ident,
        }
    }
}

pub struct Processor<'a> {
    options: &'a Options,
    env: &'a Environment,
    package_prefix: Option<ast::Package>,
    to_lower_snake: Box<naming::Naming>,
}

const PYTHON_CONTEXT: &str = "python";

impl<'a> Processor<'a> {
    pub fn new(options: &'a Options,
               env: &'a Environment,
               package_prefix: Option<ast::Package>)
               -> Processor<'a> {
        Processor {
            options: options,
            env: env,
            package_prefix: package_prefix,
            to_lower_snake: naming::SnakeCase::new().to_lower_snake(),
        }
    }

    pub fn is_native(&self, ty: &ast::Type) -> bool {
        match *ty {
            ast::Type::I32 | ast::Type::U32 => true,
            ast::Type::I64 | ast::Type::U64 => true,
            ast::Type::Float | ast::Type::Double => true,
            ast::Type::String => true,
            ast::Type::Any => true,
            ast::Type::Tuple(ref elements) => elements.iter().all(|e| self.is_native(&e.ty)),
            ast::Type::Array(ref inner) => self.is_native(inner),
            _ => false,
        }
    }

    fn ident(&self, name: &str) -> String {
        if let Some(ref id_converter) = self.options.id_converter {
            id_converter.convert(name)
        } else {
            name.to_owned()
        }
    }

    fn custom_name(&self, package: &ast::Package, custom: &str) -> Name {
        let package = self.package(package);
        let key = &(package.clone(), custom.to_owned());
        let _ = self.env.types.get(key);
        Name::local(&custom).as_name()
    }

    fn used_name(&self, package: &ast::Package, used: &str, custom: &str) -> Result<Name> {
        let package = self.env.lookup_used(package, used)?;
        let package = self.package(package);
        let package = package.parts.join(".");
        Ok(Name::imported_alias(&package, &custom, used).as_name())
    }

    pub fn encode(&self,
                  package: &ast::Package,
                  ty: &ast::Type,
                  stmt: Statement)
                  -> Result<Statement> {
        // TODO: do not skip conversion if strict type checking is enabled
        if self.is_native(ty) {
            return Ok(stmt);
        }

        let stmt = match *ty {
            ast::Type::I32 | ast::Type::U32 => stmt,
            ast::Type::I64 | ast::Type::U64 => stmt,
            ast::Type::Float | ast::Type::Double => stmt,
            ast::Type::String => stmt,
            ast::Type::Any => stmt,
            ast::Type::Custom(ref _custom) => python_stmt![stmt, ".encode()"],
            ast::Type::UsedType(ref _used, ref _custom) => python_stmt![stmt, ".encode()"],
            ast::Type::Array(ref inner) => {
                let inner = self.encode(package, inner, python_stmt!["v"])?;
                python_stmt!["map(lambda v: ", inner, ", ", stmt, ")"]
            }
            _ => stmt,
        };

        Ok(stmt)
    }

    pub fn decode(&self,
                  package: &ast::Package,
                  ty: &ast::Type,
                  stmt: Statement)
                  -> Result<Statement> {
        // TODO: do not skip conversion if strict type checking is enabled
        if self.is_native(ty) {
            return Ok(stmt);
        }

        let stmt = match *ty {
            ast::Type::I32 | ast::Type::U32 => stmt,
            ast::Type::I64 | ast::Type::U64 => stmt,
            ast::Type::Float | ast::Type::Double => stmt,
            ast::Type::String => stmt,
            ast::Type::Any => stmt,
            ast::Type::Custom(ref custom) => {
                let name = self.custom_name(package, custom);
                python_stmt![name, ".decode(", stmt, ")"]
            }
            ast::Type::UsedType(ref used, ref custom) => {
                let name = self.used_name(package, used, custom)?;
                python_stmt![name, ".decode(", stmt, ")"]
            }
            ast::Type::Array(ref inner) => {
                let inner = self.decode(package, inner, python_stmt!["v"])?;
                python_stmt!["map(lambda v: ", inner, ", ", stmt, ")"]
            }
            _ => stmt,
        };

        Ok(stmt)
    }


    /// Build the java package of a given package.
    ///
    /// This includes the prefixed configured in `self.options`, if specified.
    fn package(&self, package: &ast::Package) -> ast::Package {
        self.package_prefix
            .clone()
            .map(|prefix| prefix.join(package))
            .unwrap_or_else(|| package.clone())
    }

    fn build_constructor(&self, fields: &Vec<Field>) -> MethodSpec {
        let mut constructor = MethodSpec::new("__init__");
        constructor.push_argument(python_stmt!["self"]);

        for field in fields {
            constructor.push_argument(python_stmt![&field.ident]);
            constructor.push(python_stmt!["self.", &field.ident, " = ", &field.ident]);
        }

        constructor
    }

    fn process_type<L>(&self,
                       package: &ast::Package,
                       ty: &ast::TypeDecl,
                       listeners: &L)
                       -> Result<ClassSpec>
        where L: Listeners
    {
        match ty.value {
            ast::Type::Tuple(ref elements) => {
                let mut class = ClassSpec::new(&ty.name);
                let mut fields: Vec<Field> = Vec::new();

                for (index, element) in elements.iter().enumerate() {
                    let index_name = match index {
                        0 => "first".to_owned(),
                        1 => "second".to_owned(),
                        2 => "third".to_owned(),
                        n => format!("field{}", n),
                    };

                    let name = element.name.clone().unwrap_or(index_name);
                    let ident = self.ident(&name);
                    fields.push(Field::new(ast::Modifier::Required, element.ty.clone(), name, ident));
                }

                class.push(self.build_constructor(&fields));

                // TODO: make configurable
                if false {
                    for getter in self.build_getters(&fields)? {
                        class.push(&getter);
                    }
                }

                listeners.tuple_added(self, package, &fields, &mut class)?;
                Ok(class)
            }
            _ => Err(format!("unsupported type: {:?}", ty).into()),
        }
    }

    fn build_getters(&self, fields: &Vec<Field>) -> Result<Vec<MethodSpec>> {
        let mut result = Vec::new();

        for field in fields {
            let name = self.to_lower_snake.convert(&field.name);
            let getter_name = format!("get_{}", name);
            let mut method_spec = MethodSpec::new(&getter_name);
            method_spec.push_argument(python_stmt!["self"]);
            method_spec.push(python_stmt!["return self.", name]);
            result.push(method_spec);
        }

        Ok(result)
    }

    fn process_message<L>(&self,
                          package: &ast::Package,
                          message: &ast::MessageDecl,
                          listeners: &L)
                          -> Result<ClassSpec>
        where L: Listeners
    {
        let mut class = ClassSpec::new(&message.name);
        let mut fields = Vec::new();

        for member in &message.members {
            if let ast::MessageMember::Field(ref field, _) = *member {
                let ident = self.ident(&field.name);

                fields.push(Field::new(field.modifier.clone(),
                                       field.ty.clone(),
                                       field.name.clone(),
                                       ident));

                continue;
            }
        }

        let constructor = self.build_constructor(&fields);
        class.push(&constructor);

        // TODO: make configurable
        if false {
            for getter in self.build_getters(&fields)? {
                class.push(getter);
            }
        }

        for member in &message.members {
            if let ast::MessageMember::Code(ref context, ref content, _) = *member {
                if context == PYTHON_CONTEXT {
                    class.push(ElementSpec::Literal(content.clone()));
                }

                continue;
            }
        }

        listeners.class_added(self, package, &fields, &mut class)?;
        Ok(class)
    }

    fn process_interface<L>(&self,
                            package: &ast::Package,
                            interface: &ast::InterfaceDecl,
                            listeners: &L)
                            -> Result<Vec<ClassSpec>>
        where L: Listeners
    {
        let mut classes = Vec::new();

        let mut interface_spec = ClassSpec::new(&interface.name);

        listeners.interface_added(self, package, interface, &mut interface_spec)?;

        let mut interface_fields: Vec<Field> = Vec::new();

        for member in &interface.members {
            if let ast::InterfaceMember::Field(ref field, _) = *member {
                let ident = self.ident(&field.name);

                interface_fields.push(Field::new(field.modifier.clone(),
                                                 field.ty.clone(),
                                                 field.name.clone(),
                                                 ident));

                continue;
            }

            if let ast::InterfaceMember::Code(ref context, ref content, _) = *member {
                if context == PYTHON_CONTEXT {
                    interface_spec.push(ElementSpec::Literal(content.clone()));
                }

                continue;
            }
        }

        classes.push(interface_spec);

        for (_, ref sub_type) in &interface.sub_types {
            let mut class = ClassSpec::new(&sub_type.name);
            class.extends(Name::local(&interface.name));

            let mut fields = interface_fields.clone();

            for member in &sub_type.members {
                if let ast::SubTypeMember::Field(ref field) = *member {
                    let ident = self.ident(&field.name);

                    fields.push(Field::new(field.modifier.clone(),
                                           field.ty.clone(),
                                           field.name.clone(),
                                           ident));

                    continue;
                }
            }

            let constructor = self.build_constructor(&fields);
            class.push(&constructor);

            // TODO: make configurable
            if false {
                for getter in self.build_getters(&fields)? {
                    class.push(&getter);
                }
            }

            for member in &sub_type.members {
                if let ast::SubTypeMember::Code(ref context, ref content, _) = *member {
                    if context == PYTHON_CONTEXT {
                        class.push(ElementSpec::Literal(content.clone()));
                    }

                    continue;
                }
            }

            listeners.class_added(self, package, &fields, &mut class)?;
            classes.push(class);
        }

        Ok(classes)
    }

    fn populate_files<L>(&self, listeners: &L) -> Result<HashMap<&ast::Package, FileSpec>>
        where L: Listeners
    {
        let mut files = HashMap::new();

        // Process all types discovered so far.
        for (&(ref package, _), decl) in &self.env.types {
            let class_specs = match *decl {
                ast::Decl::Interface(ref interface) => {
                    self.process_interface(package, interface, listeners)?
                }
                ast::Decl::Message(ref message) => {
                    vec![self.process_message(package, message, listeners)?]
                }
                ast::Decl::Type(ref ty) => vec![self.process_type(package, ty, listeners)?],
            };

            match files.entry(package) {
                Entry::Vacant(entry) => {
                    let mut file_spec = FileSpec::new();

                    for class_spec in class_specs {
                        file_spec.push(class_spec);
                    }

                    entry.insert(file_spec);
                }
                Entry::Occupied(entry) => {
                    let mut file_spec = entry.into_mut();

                    for class_spec in class_specs {
                        file_spec.push(class_spec);
                    }
                }
            }
        }

        Ok(files)
    }

    fn setup_module_path(&self, root_dir: &PathBuf, package: &ast::Package) -> Result<PathBuf> {
        let package = self.package(package);

        let mut full_path = root_dir.to_owned();
        let mut iter = package.parts.iter().peekable();

        while let Some(part) = iter.next() {
            full_path = full_path.join(part);

            if iter.peek().is_none() {
                continue;
            }

            let init_path = full_path.join(INIT_PY);

            if !init_path.is_file() {
                if !full_path.is_dir() {
                    debug!("+dir: {}", full_path.display());
                    fs::create_dir_all(&full_path)?;
                }

                debug!("+init: {}", init_path.display());
                File::create(init_path)?;
            }
        }

        // path to final file
        full_path.set_extension(EXT);
        Ok(full_path)
    }

    fn write_files(&self, files: HashMap<&ast::Package, FileSpec>) -> Result<()> {
        let root_dir = &self.options.out_path;

        for (package, file_spec) in files {
            let full_path = self.setup_module_path(root_dir, package)?;

            debug!("+module: {}", full_path.display());

            let out = file_spec.format();
            let mut f = File::create(full_path)?;
            let bytes = out.into_bytes();

            f.write_all(&bytes)?;
            f.flush()?;
        }

        Ok(())
    }

    pub fn process<L>(&self, listeners: &L) -> Result<()>
        where L: Listeners
    {
        let files = self.populate_files(listeners)?;
        self.write_files(files)
    }
}