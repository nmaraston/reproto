//! # Helper trait to implement match-based decoding

use core::*;
use super::container::Container;
use super::decode::Decode;
use super::errors::*;
use super::value_builder::{ValueBuilder, ValueBuilderEnv};
use super::variables::Variables;

pub trait MatchDecode
    where Self: ValueBuilder<Stmt = <Self as Decode>::Stmt>,
          Self: Decode
{
    type Elements: Container;

    fn new_elements(&self) -> Self::Elements;

    fn match_value(&self,
                   data: &<Self as Decode>::Stmt,
                   value: &RpValue,
                   value_stmt: <Self as Decode>::Stmt,
                   result: &RpValue,
                   result_stmt: <Self as Decode>::Stmt)
                   -> Result<Self::Elements>;

    fn match_type(&self,
                  type_id: &RpTypeId,
                  data: &<Self as Decode>::Stmt,
                  kind: &RpMatchKind,
                  variable: &str,
                  decode: <Self as Decode>::Stmt,
                  result: <Self as Decode>::Stmt,
                  value: &RpByTypeMatch)
                  -> Result<Self::Elements>;

    fn decode_by_value(&self,
                       type_id: &RpTypeId,
                       match_decl: &RpMatchDecl,
                       data: &<Self as Decode>::Stmt)
                       -> Result<Option<Self::Elements>> {
        if match_decl.by_value.is_empty() {
            return Ok(None);
        }

        let variables = Variables::new();

        let mut elements = self.new_elements();

        for &(ref value, ref result) in &match_decl.by_value {
            let value_stmt = self.value(&ValueBuilderEnv {
                    value: value,
                    package: &type_id.package,
                    ty: None,
                    variables: &variables,
                })?;

            let result_stmt = self.value(&ValueBuilderEnv {
                    value: &result.instance,
                    package: &type_id.package,
                    ty: Some(&RpType::Name(type_id.name.clone())),
                    variables: &variables,
                })?;

            elements.push(&self.match_value(data, value, value_stmt, &result.instance, result_stmt)?);
        }

        Ok(Some(elements))
    }

    fn decode_by_type(&self,
                      type_id: &RpTypeId,
                      match_decl: &RpMatchDecl,
                      data: &<Self as Decode>::Stmt)
                      -> Result<Option<Self::Elements>> {
        if match_decl.by_type.is_empty() {
            return Ok(None);
        }

        let mut elements = self.new_elements();

        for &(ref kind, ref result) in &match_decl.by_type {
            let variable = &result.variable.name;

            let mut variables = Variables::new();
            variables.insert(variable.clone(), &result.variable.ty);

            let decode = self.decode(type_id, &result.variable.pos, &result.variable.ty, data)?;

            let result_value = self.value(&ValueBuilderEnv {
                    value: &result.instance,
                    package: &type_id.package,
                    ty: Some(&RpType::Name(type_id.name.clone())),
                    variables: &variables,
                })?;

            elements.push(&self.match_type(type_id, data, kind, variable, decode, result_value, result)?);
        }

        Ok(Some(elements))
    }
}
