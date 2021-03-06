extern crate inflector;
extern crate linked_hash_map;
extern crate reproto_ast as ast;
extern crate reproto_core as core;
extern crate serde;
extern crate serde_json;
extern crate serde_yaml;

mod sir;
mod json;
mod yaml;
mod format;
mod utils;

pub use self::format::Format;
pub use self::json::Json;
pub use self::yaml::Yaml;
use ast::{Attribute, AttributeItem, Decl, Field, InterfaceBody, Item, Name, SubType, TupleBody,
          Type, TypeBody, TypeMember, Value};
use core::{Loc, Object, Pos, RpModifier, RpPackage, DEFAULT_TAG};
use core::errors::Result;
use inflector::cases::pascalcase::to_pascal_case;
use inflector::cases::snakecase::to_snake_case;
use linked_hash_map::LinkedHashMap;
use sir::{FieldSir, Sir, SubTypeSir};
use std::borrow::Cow;
use std::cmp;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash;
use std::ops;
use std::rc::Rc;

#[derive(Debug)]
pub struct Derive {
    root_name: String,
    format: Box<format::Format>,
    package_prefix: Option<RpPackage>,
}

#[derive(Debug, Clone)]
struct Context<'a> {
    path: Vec<String>,
    package_prefix: Option<&'a RpPackage>,
}

impl<'a> Context<'a> {
    /// Extract the 'local name' (last component).
    fn ident(&self) -> Result<&str> {
        if let Some(ident) = self.path.iter().last() {
            Ok(ident.as_str())
        } else {
            Err(format!("No last component in name").into())
        }
    }

    /// Join this context with another path component.
    fn join(&self, name: String) -> Context<'a> {
        let mut path = self.path.iter().cloned().collect::<Vec<_>>();
        path.push(name);

        Context {
            path: path,
            package_prefix: self.package_prefix.clone(),
        }
    }

    /// Constructs an ``NAme`.
    fn name(&self) -> Name {
        Name::Absolute {
            prefix: None,
            parts: self.path.clone(),
        }
    }
}

impl Derive {
    pub fn new(
        root_name: String,
        format: Box<format::Format>,
        package_prefix: Option<RpPackage>,
    ) -> Derive {
        Derive {
            root_name: root_name,
            format: format,
            package_prefix: package_prefix,
        }
    }
}

type TypesCache = HashMap<Sir, Name>;

/// An opaque data structure, well all instances are equal but can contain different data.
#[derive(Debug, Clone)]
pub struct Opaque<T> {
    content: T,
}

impl<T> Opaque<T> {
    pub fn new(content: T) -> Self {
        Self { content: content }
    }
}

impl<T> cmp::PartialEq for Opaque<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<T> cmp::Eq for Opaque<T> {}

impl<T> hash::Hash for Opaque<T> {
    fn hash<H: hash::Hasher>(&self, _state: &mut H) {}
}

impl<T> ops::Deref for Opaque<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

impl<T> ops::DerefMut for Opaque<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.content
    }
}

struct FieldInit<'a> {
    pos: &'a Pos,
    ctx: Context<'a>,
    types: &'a mut TypesCache,
}

impl<'a> FieldInit<'a> {
    fn new(pos: &'a Pos, ctx: Context<'a>, types: &'a mut TypesCache) -> FieldInit<'a> {
        FieldInit {
            pos: pos,
            ctx: ctx,
            types: types,
        }
    }

    fn init<'input>(
        self,
        original_name: String,
        sir: &FieldSir,
        members: &mut Vec<TypeMember<'input>>,
    ) -> Result<Item<'input, Field<'input>>> {
        let mut comment = Vec::new();

        let name = to_snake_case(&original_name);

        let ty = match sir.field {
            Sir::Boolean => Type::Boolean,
            Sir::Float => Type::Float,
            Sir::Double => Type::Double,
            Sir::I64(ref examples) => {
                format_comment(&mut comment, examples)?;
                Type::Signed { size: 64 }
            }
            Sir::U64(ref examples) => {
                format_comment(&mut comment, examples)?;
                Type::Unsigned { size: 64 }
            }
            Sir::String(ref examples) => {
                format_comment(&mut comment, examples)?;
                Type::String
            }
            Sir::DateTime(ref examples) => {
                format_comment(&mut comment, examples)?;
                Type::DateTime
            }
            Sir::Any => Type::Any,
            Sir::Array(ref inner) => {
                let field = FieldSir {
                    optional: false,
                    field: (**inner).clone(),
                };

                let f = FieldInit::new(&self.pos, self.ctx.clone(), self.types).init(
                    name.clone(),
                    &field,
                    members,
                )?;

                Type::Array {
                    inner: Box::new(f.item.ty.clone()),
                }
            }
            ref sir => {
                let ctx = self.ctx.join(to_pascal_case(&name));

                let name = if let Some(name) = self.types.get(sir).cloned() {
                    name
                } else {
                    let name = ctx.name();

                    self.types.insert(sir.clone(), name.clone());

                    let decl = DeclDeriver {
                        pos: &self.pos,
                        ctx: ctx.clone(),
                        types: self.types,
                    }.derive(sir)?;

                    members.push(TypeMember::InnerDecl(decl));

                    name
                };

                Type::Name { name: name }
            }
        };

        let field_as = if name != original_name {
            Some(original_name)
        } else {
            None
        };

        let field = Field {
            modifier: if sir.optional {
                RpModifier::Optional
            } else {
                RpModifier::Required
            },
            name: name.clone().into(),
            ty: ty,
            field_as: field_as,
        };

        // field referencing inner declaration
        return Ok(Item {
            comment: comment,
            attributes: Vec::new(),
            item: Loc::new(field, self.pos.clone()),
        });

        /// Format comments and attach examples.
        fn format_comment<T>(out: &mut Vec<Cow<'static, str>>, examples: &[T]) -> Result<()>
        where
            T: serde::Serialize + fmt::Debug,
        {
            out.push(format!("## Examples").into());
            out.push("".to_string().into());

            out.push(format!("```json").into());

            let mut seen = HashSet::new();

            for example in examples.iter() {
                let string = serde_json::to_string_pretty(example)
                    .map_err(|e| format!("Failed to convert to JSON: {}: {:?}", e, example))?;

                if !seen.contains(&string) {
                    seen.insert(string.clone());
                    out.push(string.into());
                }
            }

            out.push(format!("```").into());

            Ok(())
        }
    }
}

struct DeclDeriver<'a> {
    pos: &'a Pos,
    ctx: Context<'a>,
    types: &'a mut TypesCache,
}

impl<'a> DeclDeriver<'a> {
    /// Derive a declaration from the given JSON.
    fn derive<'s, 'input>(self, sir: &'s Sir) -> Result<Decl<'input>> {
        let decl = match *sir {
            Sir::Tuple(ref array) => {
                let tuple = TupleRefiner {
                    pos: &self.pos,
                    ctx: self.ctx,
                    types: self.types,
                }.derive(array)?;

                Decl::Tuple(tuple)
            }
            Sir::Object(ref object) => {
                let type_ = TypeRefiner {
                    pos: &self.pos,
                    ctx: self.ctx,
                    types: self.types,
                }.derive(object)?;

                Decl::Type(type_)
            }
            Sir::Interface(ref type_field, ref sub_types) => {
                let interface = InterfaceRefiner {
                    pos: &self.pos,
                    ctx: self.ctx,
                    types: self.types,
                }.derive(type_field, sub_types)?;

                Decl::Interface(interface)
            }
            // For arrays, only generate the inner type.
            Sir::Array(ref inner) => self.derive(inner)?,
            ref value => return Err(format!("Unexpected JSON value: {:?}", value).into()),
        };

        Ok(decl)
    }
}

struct TypeRefiner<'a> {
    pos: &'a Pos,
    ctx: Context<'a>,
    types: &'a mut TypesCache,
}

impl<'a> TypeRefiner<'a> {
    /// Derive an struct body from the given input array.
    fn derive<'input>(
        &mut self,
        object: &LinkedHashMap<String, FieldSir>,
    ) -> Result<Item<'input, TypeBody<'input>>> {
        let mut body = TypeBody {
            name: self.ctx.ident()?.to_string().into(),
            members: Vec::new(),
        };

        self.init(&mut body, object)?;

        Ok(Item {
            comment: Vec::new(),
            attributes: Vec::new(),
            item: Loc::new(body, self.pos.clone()),
        })
    }

    fn init<'input>(
        &mut self,
        base: &mut TypeBody<'input>,
        object: &LinkedHashMap<String, FieldSir>,
    ) -> Result<()> {
        for (name, added) in object.iter() {
            let field = FieldInit::new(&self.pos, self.ctx.clone(), self.types).init(
                name.to_string(),
                added,
                &mut base.members,
            )?;

            base.members.push(TypeMember::Field(field));
        }

        Ok(())
    }
}

struct SubTypeRefiner<'a> {
    pos: &'a Pos,
    ctx: Context<'a>,
    types: &'a mut TypesCache,
}

impl<'a> SubTypeRefiner<'a> {
    /// Derive an struct body from the given input array.
    fn derive<'input>(&mut self, sub_type: &SubTypeSir) -> Result<Item<'input, SubType<'input>>> {
        let mut body = SubType {
            name: Loc::new(self.ctx.ident()?.to_string().into(), self.pos.clone()),
            members: vec![],
            alias: None,
        };

        self.init(&mut body, sub_type)?;

        Ok(Item {
            comment: Vec::new(),
            attributes: Vec::new(),
            item: Loc::new(body, self.pos.clone()),
        })
    }

    fn init<'input>(&mut self, base: &mut SubType<'input>, sub_type: &SubTypeSir) -> Result<()> {
        if sub_type.name.as_str() != base.name.as_ref() {
            base.alias = Some(Loc::new(
                Value::String(sub_type.name.to_string()),
                self.pos.clone(),
            ));
        }

        for (field_name, field_value) in &sub_type.structure {
            let field = FieldInit::new(&self.pos, self.ctx.clone(), self.types).init(
                field_name.to_string(),
                field_value,
                &mut base.members,
            )?;

            base.members.push(TypeMember::Field(field));
        }

        Ok(())
    }
}

struct InterfaceRefiner<'a> {
    pos: &'a Pos,
    ctx: Context<'a>,
    types: &'a mut TypesCache,
}

impl<'a> InterfaceRefiner<'a> {
    /// Derive an struct body from the given input array.
    fn derive<'input>(
        &mut self,
        tag: &str,
        sub_types: &[SubTypeSir],
    ) -> Result<Item<'input, InterfaceBody<'input>>> {
        let mut attributes = Vec::new();

        if tag != DEFAULT_TAG {
            let name = Loc::new("type_info".into(), self.pos.clone());
            let mut values = Vec::new();

            values.push(AttributeItem::NameValue {
                name: Loc::new("type".into(), self.pos.clone()),
                value: Loc::new(Value::String("type".to_string()), self.pos.clone()),
            });

            let a = Attribute::List(name, values);

            attributes.push(Loc::new(a, self.pos.clone()));
        };

        let mut body = InterfaceBody {
            name: self.ctx.ident()?.to_string().into(),
            members: Vec::new(),
            sub_types: Vec::new(),
        };

        self.init(&mut body, sub_types)?;

        Ok(Item {
            comment: Vec::new(),
            attributes: attributes,
            item: Loc::new(body, self.pos.clone()),
        })
    }

    fn init<'input>(
        &mut self,
        base: &mut InterfaceBody<'input>,
        sub_types: &[SubTypeSir],
    ) -> Result<()> {
        for st in sub_types {
            let ident = to_pascal_case(&st.name);
            let ctx = self.ctx.join(ident.clone());

            let sub_type = SubTypeRefiner {
                pos: self.pos,
                ctx: ctx,
                types: self.types,
            }.derive(st)?;

            base.sub_types.push(sub_type);
        }

        Ok(())
    }
}

struct TupleRefiner<'a> {
    pos: &'a Pos,
    ctx: Context<'a>,
    types: &'a mut TypesCache,
}

impl<'a> TupleRefiner<'a> {
    /// Derive an tuple body from the given input array.
    fn derive<'input>(&mut self, array: &[FieldSir]) -> Result<Item<'input, TupleBody<'input>>> {
        let mut body = TupleBody {
            name: self.ctx.ident()?.to_string().into(),
            members: Vec::new(),
        };

        self.init(&mut body, array)?;

        Ok(Item {
            comment: Vec::new(),
            attributes: Vec::new(),
            item: Loc::new(body, self.pos.clone()),
        })
    }

    fn init<'input>(&mut self, base: &mut TupleBody<'input>, array: &[FieldSir]) -> Result<()> {
        for (index, added) in array.iter().enumerate() {
            let field = FieldInit::new(&self.pos, self.ctx.clone(), self.types).init(
                format!("field_{}", index),
                added,
                &mut base.members,
            )?;

            base.members.push(TypeMember::Field(field));
        }

        Ok(())
    }
}

/// Derive a declaration from the given input.
pub fn derive<'input>(derive: Derive, object: &'input Object) -> Result<Decl<'input>> {
    let Derive {
        root_name,
        format,
        package_prefix,
    } = derive;

    let sir = format.decode(object)?;

    let pos: Pos = (Rc::new(object.clone_object()), 0, 0).into();

    let mut types = HashMap::new();

    let ctx = Context {
        path: vec![root_name],
        package_prefix: package_prefix.as_ref(),
    };

    let decl = DeclDeriver {
        pos: &pos,
        ctx: ctx,
        types: &mut types,
    }.derive(&sir)?;

    Ok(decl)
}

#[cfg(test)]
mod tests {
    use super::{derive, Derive, Json};
    use ast::Decl;
    use core::BytesObject;
    use std::sync::Arc;

    fn input<T>(input: &str, test: T)
    where
        T: Fn(Decl) -> (),
    {
        let object = BytesObject::new(
            "test".to_string(),
            Arc::new(input.as_bytes().iter().cloned().collect()),
        );

        let derive_config = Derive {
            root_name: "Generator".to_string(),
            format: Box::new(Json),
            package_prefix: None,
        };

        test(derive(derive_config, &object).expect("bad derive"))
    }

    #[test]
    fn simple_declaration() {
        input(r#"{"id": 42, "name": "Oscar"}"#, |decl| {
            let ty = match decl {
                Decl::Type(ty) => ty,
                other => panic!("expected type, got: {:?}", other),
            };

            assert_eq!(2, ty.members.len());
        });
    }

    #[test]
    fn test_interface() {
        input(
            r#"[
    {"kind": "dragon", "name": "Stephen", "age": 4812, "fire": "blue"},
    {"kind": "knight", "name": "Olivia", "armor": "Unobtanium"}
]"#,
            |decl| {
                let intf = match decl {
                    Decl::Interface(intf) => intf,
                    other => panic!("expected interface, got: {:?}", other),
                };

                assert_eq!(2, intf.sub_types.len());
            },
        );
    }
}
