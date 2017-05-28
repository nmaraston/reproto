use backend::models as m;
use num_bigint::BigInt;
use pest::prelude::*;
use std::collections::LinkedList;
use super::ast;
use super::errors::*;

/// Check if character is an indentation character.
fn is_indent(c: char) -> bool {
    match c {
        ' ' | '\t' => true,
        _ => false,
    }
}

/// Find the number of whitespace characters that the given string is indented.
fn find_indent(input: &str) -> Option<usize> {
    input.chars().enumerate().find(|&(_, c)| !is_indent(c)).map(|(i, _)| i)
}

fn code_block_indent(input: &str) -> Option<(usize, usize, usize)> {
    let mut indent: Option<usize> = None;

    let mut start = 0;
    let mut end = 0;

    let mut first_line = false;

    for (line_no, line) in input.lines().enumerate() {
        if let Some(current) = find_indent(line) {
            end = line_no + 1;

            if indent.map(|i| i > current).unwrap_or(true) {
                indent = Some(current);
            }

            first_line = true;
        } else {
            if !first_line {
                start += 1;
            }
        }
    }

    indent.map(|indent| (indent, start, end - start))
}

/// Strip common indent from all input lines.
fn strip_code_block(input: &str) -> Vec<String> {
    if let Some((indent, empty_start, len)) = code_block_indent(input) {
        input.lines()
            .skip(empty_start)
            .take(len)
            .map(|line| {
                if line.len() < indent {
                    line.to_owned()
                } else {
                    (&line[indent..]).to_owned()
                }
            })
            .collect()
    } else {
        input.lines().map(ToOwned::to_owned).collect()
    }
}

/// Decode an escaped string.
fn decode_escaped_string(input: &str) -> Result<String> {
    let mut out = String::new();
    let mut it = input.chars().skip(1).peekable();

    loop {
        let c = match it.next() {
            None => break,
            Some(c) => c,
        };

        // strip end quote
        if it.peek().is_none() {
            break;
        }

        if c == '\\' {
            let escaped = match it.next().ok_or("expected character")? {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'u' => decode_unicode4(&mut it)?,
                _ => return Err(ErrorKind::InvalidEscape.into()),
            };

            out.push(escaped);
            continue;
        }

        out.push(c);
    }

    Ok(out)
}

/// Decode the next four characters as a unicode escape sequence.
fn decode_unicode4(it: &mut Iterator<Item = char>) -> Result<char> {
    let mut res = 0u32;

    for x in 0..4u32 {
        let c = it.next().ok_or("expected hex character")?.to_string();
        let c = u32::from_str_radix(&c, 16)?;
        res += c << (4 * (3 - x));
    }

    Ok(::std::char::from_u32(res).ok_or("expected valid character")?)
}

impl_rdp! {
    grammar! {
        file = _{ package_decl ~ use_decl* ~ decl* ~ eoi }
        decl = { type_decl | interface_decl | tuple_decl | enum_decl }

        use_decl = { use_keyword ~ package_ident ~ use_as? ~ semi_colon }
        use_as = { as_keyword ~ identifier }

        package_decl = { package_keyword ~ package_ident ~ semi_colon }

        type_decl = { type_keyword ~ type_identifier ~ left_curly ~ type_body ~ right_curly }
        type_body = _{ option_decl* ~ member* }

        tuple_decl = { tuple_keyword ~ type_identifier ~ left_curly ~ tuple_body ~ right_curly }
        tuple_body = _{ option_decl* ~ member* }

        interface_decl = { interface_keyword ~ type_identifier ~ left_curly ~ interface_body ~ right_curly }
        interface_body = _{ option_decl* ~ member* ~ sub_type* }

        enum_decl = { enum_keyword ~ type_identifier ~ left_curly ~ enum_body ~ right_curly }
        enum_body = _{ enum_value ~ (comma ~ enum_value)* ~ semi_colon ~ option_decl* ~ member* }

        sub_type = { type_identifier ~ left_curly ~ option_decl* ~ member* ~ right_curly }

        member = { field | code_block }
        field = { identifier ~ optional? ~ colon ~ type_spec ~ semi_colon }
        code_block = @{ identifier ~ whitespace* ~ code_start ~ code_body ~ code_end }
        code_body = { (!(["}}"]) ~ any)* }

        enum_value = { identifier ~ (left_paren ~ (value ~ (comma ~ value)*) ~ right_paren)? }
        option_decl = { identifier ~ (option_value ~ (comma ~ option_value)*) ~ semi_colon }

        option_value = { string | integer | unsigned | identifier }

        package_ident = @{ identifier ~ (["."] ~ identifier)* }

        type_spec = _{
            float_type |
            double_type |
            signed_type |
            unsigned_type |
            boolean_type |
            string_type |
            bytes_type |
            any_type |
            map_type |
            array_type |
            used_type |
            custom_type
        }

        // Types
        float_type = @{ ["float"] }
        double_type = @{ ["double"] }
        signed_type = @{ ["signed"] ~ type_bits? }
        unsigned_type = @{ ["unsigned"] ~ type_bits? }
        boolean_type = @{ ["boolean"] }
        string_type = @{ ["string"] }
        bytes_type = @{ ["bytes"] }
        any_type = @{ ["any"] }
        map_type = { left_curly ~ type_spec ~ colon ~ type_spec ~ right_curly }
        array_type = { ["["] ~ type_spec ~ ["]"] }
        used_type = @{ identifier ~ ["."] ~ type_identifier }
        custom_type = { type_identifier }

        // Keywords and tokens
        enum_keyword = @{ ["enum"] }
        use_keyword = @{ ["use"] }
        as_keyword = @{ ["as"] }
        package_keyword = @{ ["package"] }
        type_keyword = @{ ["type"] }
        tuple_keyword = @{ ["tuple"] }
        interface_keyword = @{ ["interface"] }
        comma = @{ [","] }
        colon = @{ [":"] }
        semi_colon = @{ [";"] }
        left_curly = @{ ["{"] }
        right_curly = @{ ["}"] }
        code_start = @{ ["{{"] }
        code_end = @{ ["}}"] }
        left_paren = @{ ["("] }
        right_paren = @{ [")"] }
        forward_slash = @{ ["/"] }
        optional = @{ ["?"] }

        type_bits = _{ (forward_slash ~ unsigned) }

        value = { string | decimal | integer | boolean }

        identifier = @{ (['a'..'z'] | ['A'..'Z'] | ["_"]) ~ (['0'..'9'] | ['a'..'z'] | ['A'..'Z'] | ["_"])* }
        type_identifier = @{ (['A'..'Z'] ~ ['a'..'z']*)+ }

        string  = @{ ["\""] ~ (escape | !(["\""] | ["\\"]) ~ any)* ~ ["\""] }
        escape  =  _{ ["\\"] ~ (["\""] | ["\\"] | ["/"] | ["n"] | ["r"] | ["t"] | unicode) }
        unicode =  _{ ["u"] ~ hex ~ hex ~ hex ~ hex }
        hex     =  _{ ['0'..'9'] | ['a'..'f'] }

        integer   = @{ ["-"]? ~ int }
        unsigned = @{ ["-"]? ~ int }
        decimal    = @{ ["-"]? ~ int? ~ (["."] ~ ['0'..'9']+) }
        int      =  _{ ["0"] | ['1'..'9'] ~ ['0'..'9']* }

        boolean = { ["true"] | ["false"] }

        whitespace = _{ [" "] | ["\t"] | ["\r"] | ["\n"] }

        comment = _{
            // line comment
            ( ["//"] ~ (!(["\r"] | ["\n"]) ~ any)* ~ (["\n"] | ["\r\n"] | ["\r"] | eoi) ) |
            // block comment
            ( ["/*"] ~ (!(["*/"]) ~ any)* ~ ["*/"] )
        }
    }

    process! {
        _file(&self) -> Result<ast::File> {
            (
                _: package_decl,
                _: package_keyword,
                package: _package(), _: semi_colon,
                uses: _use_list(),
                decls: _decl_list(),
            ) => {
                let package = package;
                let uses = uses?.into_iter().collect();
                let decls = decls?.into_iter().collect();

                Ok(ast::File {
                    package: package,
                    uses: uses,
                    decls: decls
                })
            },
        }

        _use_list(&self) -> Result<LinkedList<ast::Token<ast::UseDecl>>> {
            (token: use_decl, use_decl: _use_decl(), tail: _use_list()) => {
                let pos = (token.start, token.end);
                let mut tail = tail?;
                tail.push_front(ast::Token::new(use_decl, pos));
                Ok(tail)
            },

            () => Ok(LinkedList::new()),
        }

        _use_decl(&self) -> ast::UseDecl {
            (_: use_keyword, package: _package(), alias: _use_as(), _: semi_colon) => {
                ast::UseDecl {
                    package: package,
                    alias: alias,
                }
            }
        }

        _use_as(&self) -> Option<String> {
            (_: use_as, _: as_keyword, &alias: identifier) => Some(alias.to_owned()),
            () => None,
        }

        _package(&self) -> ast::Token<m::Package> {
            (token: package_ident, idents: _ident_list()) => {
                let pos = (token.start, token.end);
                let idents = idents;
                let package = m::Package::new(idents.into_iter().collect());
                ast::Token::new(package, pos)
            },
        }

        _decl_list(&self) -> Result<LinkedList<ast::Token<ast::Decl>>> {
            (token: decl, value: _decl(), tail: _decl_list()) => {
                let mut tail = tail?;
                let pos = (token.start, token.end);
                tail.push_front(ast::Token::new(value?, pos));
                Ok(tail)
            },

            () => Ok(LinkedList::new()),
        }

        _decl(&self) -> Result<ast::Decl> {
            (
                _: type_decl,
                _: type_keyword,
                &name: type_identifier,
                _: left_curly,
                options: _option_list(),
                members: _member_list(),
                _: right_curly,
            ) => {
                let options = ast::Options::new(options?.into_iter().collect());
                let members = members?.into_iter().collect();

                let body = ast::TypeBody {
                    name: name.to_owned(),
                    options: options,
                    members: members
                };

                Ok(ast::Decl::Type(body))
            },

            (
                _: tuple_decl,
                _: tuple_keyword,
                &name: type_identifier,
                _: left_curly,
                options: _option_list(),
                members: _member_list(),
                _: right_curly,
            ) => {
                let options = ast::Options::new(options?.into_iter().collect());
                let members = members?.into_iter().collect();

                let body = ast::TupleBody {
                    name: name.to_owned(),
                    options: options,
                    members: members,
                };

                Ok(ast::Decl::Tuple(body))
            },

            (
                _: interface_decl,
                _: interface_keyword,
                &name: type_identifier,
                _: left_curly,
                options: _option_list(),
                members: _member_list(),
                sub_types: _sub_type_list(),
                _: right_curly,
            ) => {
                let options = ast::Options::new(options?.into_iter().collect());
                let members = members?.into_iter().collect();
                let sub_types = sub_types?.into_iter().collect();

                let body = ast::InterfaceBody {
                    name: name.to_owned(),
                    options: options,
                    members: members,
                    sub_types: sub_types,
                };

                Ok(ast::Decl::Interface(body))
            },

            (
                _: enum_decl,
                _: enum_keyword,
                &name: type_identifier,
                _: left_curly,
                values: _enum_value_list(),
                _: semi_colon,
                options: _option_list(),
                members: _member_list(),
                _: right_curly,
            ) => {
                let values = values?.into_iter().collect();
                let options = ast::Options::new(options?.into_iter().collect());
                let members = members?.into_iter().collect();

                let body = ast::EnumBody {
                    name: name.to_owned(),
                    values: values,
                    options: options,
                    members: members,
                };

                Ok(ast::Decl::Enum(body))
            },
        }

        _enum_value_list(&self) -> Result<LinkedList<ast::Token<ast::EnumValue>>> {
            (token: enum_value, value: _enum_value(), _: comma, tail: _enum_value_list()) => {
                let value = value?;
                let mut tail = tail?;
                let pos = (token.start, token.end);
                tail.push_front(ast::Token::new(value, pos));
                Ok(tail)
            },

            (token: enum_value, value: _enum_value()) => {
                let value = value?;
                let pos = (token.start, token.end);

                let mut tail = LinkedList::new();
                tail.push_front(ast::Token::new(value, pos));
                Ok(tail)
            },
        }

        _enum_value(&self) -> Result<ast::EnumValue> {
            (&name: identifier, _: left_paren, values: _value_list(), _: right_paren) => {
                let arguments = values?.into_iter().collect();

                Ok(ast::EnumValue { name: name.to_owned(), arguments: arguments })
            },
        }

        _value_list(&self) -> Result<LinkedList<ast::Token<m::Value>>> {
            (token: value, value: _value(), _: comma, tail: _value_list()) => {
                let value = value?;
                let pos = (token.start, token.end);
                let mut tail = tail?;
                tail.push_front(ast::Token::new(value, pos));
                Ok(tail)
            },

            (token: value, value: _value()) => {
                let value = value?;
                let pos = (token.start, token.end);
                let mut tail = LinkedList::new();
                tail.push_front(ast::Token::new(value, pos));
                Ok(tail)
            },
        }

        _value(&self) -> Result<m::Value> {
            (&value: string) => {
                let value = decode_escaped_string(value)?;
                Ok(m::Value::String(value))
            },

            (&value: decimal) => {
                let value = value.parse::<f64>()?;
                Ok(m::Value::Decimal(value))
            },

            (&value: integer) => {
                Ok(m::Value::Integer(value.parse::<BigInt>()?))
            },

            (&value: boolean) => {
                let value = match value {
                    "true" => true,
                    "false" => false,
                    _ => panic!("should not happen"),
                };

                Ok(m::Value::Boolean(value))
            },
        }

        _option_list(&self) -> Result<LinkedList<ast::Token<ast::OptionDecl>>> {
            (token: option_decl, first: _option_decl(), _: semi_colon, tail: _option_list()) => {
                let mut tail = tail?;
                let pos = (token.start, token.end);
                tail.push_front(ast::Token::new(first?, pos));
                Ok(tail)
            },

            () => Ok(LinkedList::new()),
        }

        _option_decl(&self) -> Result<ast::OptionDecl> {
            (&name: identifier, values: _option_value_list()) => {
                let values = values?.into_iter().collect();
                Ok(ast::OptionDecl { name: name.to_owned(), values: values })
            },
        }

        _option_value_list(&self) -> Result<LinkedList<ast::Token<ast::OptionValue>>> {
            (token: option_value, value: _option_value(), _: comma, tail: _option_value_list()) => {
                let pos = (token.start, token.end);
                let mut tail = tail?;
                tail.push_front(ast::Token::new(value?, pos));
                Ok(tail)
            },

            (token: option_value, value: _option_value()) => {
                let pos = (token.start, token.end);
                let mut tail = LinkedList::new();
                tail.push_front(ast::Token::new(value?, pos));
                Ok(tail)
            },
        }

        _option_value(&self) -> Result<ast::OptionValue> {
            (&string: string) => {
                let string = decode_escaped_string(string)?;
                Ok(ast::OptionValue::String(string))
            },

            (&value: identifier) => {
                Ok(ast::OptionValue::Identifier(value.to_owned()))
            },

            (&value: integer) => {
                let value = value.parse::<i64>()?;
                Ok(ast::OptionValue::Integer(value))
            },

            (&value: unsigned) => {
                let value = value.parse::<u64>()?;
                Ok(ast::OptionValue::Integer(value as i64))
            },
        }

        _member_list(&self) -> Result<LinkedList<ast::Token<ast::Member>>> {
            (token: member, value: _member(), tail: _member_list()) => {
                let mut tail = tail?;
                let pos = (token.start, token.end);
                tail.push_front(ast::Token::new(value?, pos));
                Ok(tail)
            },

            () => Ok(LinkedList::new()),
        }

        _member(&self) -> Result<ast::Member> {
            (
                _: field,
                &name: identifier,
                modifier: _modifier(),
                _: colon,
                type_spec: _type_spec(),
                _: semi_colon,
            ) => {
                let field = ast::Field::new(modifier, name.to_owned(), type_spec?, 0);
                Ok(ast::Member::Field(field))
            },

            (
                _: code_block,
                &context: identifier,
                _: code_start,
                &content: code_body,
                _: code_end,
             ) => {
                let block = strip_code_block(content);
                Ok(ast::Member::Code(context.to_owned(), block))
            },
        }

        _sub_type_list(&self) -> Result<LinkedList<ast::Token<ast::SubType>>> {
            (token: sub_type, value: _sub_type(), tail: _sub_type_list()) => {
                let mut tail = tail?;
                let pos = (token.start, token.end);
                tail.push_front(ast::Token::new(value?, pos));
                Ok(tail)
            },

            () => {
                Ok(LinkedList::new())
            },
        }

        _sub_type(&self) -> Result<ast::SubType> {
            (
                &name: type_identifier,
                _: left_curly,
                options: _option_list(),
                members: _member_list(),
                _: right_curly,
             ) => {
                let name = name.to_owned();
                let options = ast::Options::new(options?.into_iter().collect());
                let members = members?.into_iter().collect();
                Ok(ast::SubType { name: name, options: options, members: members })
            },
        }

        _type_spec(&self) -> Result<m::Type> {
            (_: double_type) => {
                Ok(m::Type::Double)
            },

            (_: float_type) => {
                Ok(m::Type::Float)
            },

            (_: signed_type, _: forward_slash, &size: unsigned) => {
                let size = size.parse::<usize>()?;
                Ok(m::Type::Signed(Some(size)))
            },

            (_: unsigned_type, _: forward_slash, &size: unsigned) => {
                let size = size.parse::<usize>()?;
                Ok(m::Type::Unsigned(Some(size)))
            },

            (_: signed_type) => {
                Ok(m::Type::Signed(None))
            },

            (_: unsigned_type) => {
                Ok(m::Type::Unsigned(None))
            },

            (_: boolean_type) => {
                Ok(m::Type::Boolean)
            },

            (_: string_type) => {
                Ok(m::Type::String)
            },

            (_: bytes_type) => {
                Ok(m::Type::Bytes)
            },

            (_: any_type) => {
                Ok(m::Type::Any)
            },

            (_: custom_type, &name: type_identifier) => {
                Ok(m::Type::Custom(name.to_owned()))
            },

            (_: used_type, &used: identifier, &value: type_identifier) => {
                Ok(m::Type::UsedType(used.to_owned(), value.to_owned()))
            },

            (_: array_type, argument: _type_spec()) => {
                let argument = argument?;
                Ok(m::Type::Array(Box::new(argument)))
            },

            (
                _: map_type,
                _: left_curly,
                key: _type_spec(),
                _: colon,
                value: _type_spec(),
                _: right_curly
             ) => {
                let key = key?;
                let value = value?;
                Ok(m::Type::Map(Box::new(key), Box::new(value)))
            },
        }

        _modifier(&self) -> m::Modifier {
            (_: optional) => m::Modifier::Optional,
            () => m::Modifier::Required,
        }

        _ident_list(&self) -> LinkedList<String> {
            (&value: identifier, mut tail: _ident_list()) => {
                tail.push_front(value.to_owned());
                tail
            },

            () => LinkedList::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE1: &[u8] = include_bytes!("tests/file1.reproto");

    fn parse(input: &'static str) -> Rdp<StringInput> {
        Rdp::new(StringInput::new(input))
    }

    #[test]
    fn test_file1() {
        let input = ::std::str::from_utf8(FILE1).unwrap();
        let mut parser = parse(input);

        assert!(parser.file());
        assert!(parser.end());

        let file = parser._file().unwrap();

        let package = m::Package::new(vec!["foo".to_owned(), "bar".to_owned(), "baz".to_owned()]);

        assert_eq!(package, *file.package);
        assert_eq!(4, file.decls.len());
    }

    #[test]
    fn test_array() {
        let mut parser = parse("[string]");

        assert!(parser.type_spec());
        assert!(parser.end());

        let ty = parser._type_spec().unwrap();

        if let m::Type::Array(inner) = ty {
            if let m::Type::String = *inner {
                return;
            }
        }

        panic!("Expected Type::Array(Type::String)");
    }

    #[test]
    fn test_map() {
        let mut parser = parse("{string: unsigned/123}");

        assert!(parser.type_spec());
        assert!(parser.end());

        let ty = parser._type_spec().unwrap();

        // TODO: use #![feature(box_patterns)]:
        // if let m::Type::Map(box m::Type::String, box m::Type::Unsigned(size)) = ty {
        // }
        if let m::Type::Map(key, value) = ty {
            if let m::Type::String = *key {
                if let m::Type::Unsigned(size) = *value {
                    assert_eq!(Some(123usize), size);
                    return;
                }
            }
        }

        panic!("Expected Type::Array(Type::String)");
    }

    #[test]
    fn test_block_comment() {
        let mut parser = parse("/* hello \n world */");

        assert!(parser.comment());
    }

    #[test]
    fn test_line_comment() {
        let mut parser = parse("// hello world\n");

        assert!(parser.comment());
    }

    #[test]
    fn test_code_block() {
        let mut parser = parse("a { b { c } d } e");

        assert!(parser.code_body());
        assert!(parser.end());
    }

    #[test]
    fn test_code() {
        let mut parser = parse("java{{\na { b { c } d } e\n}}");

        assert!(parser.code_block());
        assert!(parser.end());
    }

    #[test]
    fn test_find_indent() {
        assert_eq!(Some(4), find_indent("   \thello"));
        assert_eq!(Some(0), find_indent("nope"));
        assert_eq!(None, find_indent(""));
        assert_eq!(None, find_indent("    "));
    }

    #[test]
    fn test_strip_code_block() {
        let result = strip_code_block("\n   hello\n  world\n\n\n again\n\n\n");
        assert_eq!(vec!["  hello", " world", "", "", "again"], result);
    }

    #[test]
    fn test_value() {
        let mut parser = parse("62.0");

        assert!(parser.value());
        assert!(parser.end());
    }

    #[test]
    fn test_interface() {
        let input = "package foo.bar; interface Foo { reserved 1, 2, 3; java {{  }} Hello { } \
                     World { } }";

        let mut parser = parse(input);

        assert!(parser.file());
        assert!(parser.end());

        let file = parser._file().unwrap();

        assert_eq!(1, file.decls.len());
    }
}
