use std::{fmt::Display, mem, str::FromStr};

use proc_macro2::{Ident, Span, TokenStream};
use proc_macro_error::abort;
use quote::{quote, ToTokens};
use syn::{parenthesized, parse::ParseStream, LitFloat, LitInt, LitStr, TypePath};

use crate::{
    feature::{
        Example, Explode, Feature, Format, Inline, MultipleOf, Name, Nullable, ParameterIn, Rename, RenameRule, Style,
        Title, Validatable, ValueType, WriteOnly, XmlAttr, RenameAll
    },
    parameter::{self, ParameterStyle},
    parse_utils, schema,
    schema_type::{SchemaFormat, SchemaType},
    type_tree::{GenericType, TypeTree},
    AnyValue,
};

pub trait ToTokensExt {
    fn to_token_stream(&self) -> TokenStream;
}

impl ToTokensExt for Vec<Feature> {
    fn to_token_stream(&self) -> TokenStream {
        self.iter().fold(TokenStream::new(), |mut tokens, item| {
            item.to_tokens(&mut tokens);
            tokens
        })
    }
}

pub trait FeaturesExt {
    fn pop_by(&mut self, op: impl FnMut(&Feature) -> bool) -> Option<Feature>;

    fn pop_value_type_feature(&mut self) -> Option<ValueType>;

    /// Pop [`Rename`] feature if exists in [`Vec<Feature>`] list.
    fn pop_rename_feature(&mut self) -> Option<Rename>;

    /// Pop [`RenameAll`] feature if exists in [`Vec<Feature>`] list.
    fn pop_rename_all_feature(&mut self) -> Option<RenameAll>;

    /// Extract [`XmlAttr`] feature for given `type_tree` if it has generic type [`GenericType::Vec`]
    fn extract_vec_xml_feature(&mut self, type_tree: &TypeTree) -> Option<Feature>;
}

impl FeaturesExt for Vec<Feature> {
    fn pop_by(&mut self, op: impl FnMut(&Feature) -> bool) -> Option<Feature> {
        self.iter().position(op).map(|index| self.swap_remove(index))
    }

    fn pop_value_type_feature(&mut self) -> Option<ValueType> {
        self.pop_by(|feature| matches!(feature, Feature::ValueType(_)))
            .and_then(|feature| match feature {
                Feature::ValueType(value_type) => Some(value_type),
                _ => None,
            })
    }

    fn pop_rename_feature(&mut self) -> Option<Rename> {
        self.pop_by(|feature| matches!(feature, Feature::Rename(_)))
            .and_then(|feature| match feature {
                Feature::Rename(rename) => Some(rename),
                _ => None,
            })
    }

    fn pop_rename_all_feature(&mut self) -> Option<RenameAll> {
        self.pop_by(|feature| matches!(feature, Feature::RenameAll(_)))
            .and_then(|feature| match feature {
                Feature::RenameAll(rename_all) => Some(rename_all),
                _ => None,
            })
    }

    fn extract_vec_xml_feature(&mut self, type_tree: &TypeTree) -> Option<Feature> {
        self.iter_mut().find_map(|feature| match feature {
            Feature::XmlAttr(xml_feature) => {
                let (vec_xml, value_xml) = xml_feature.split_for_vec(type_tree);

                // replace the original xml attribute with splitted value xml
                if let Some(mut xml) = value_xml {
                    mem::swap(xml_feature, &mut xml)
                }

                vec_xml.map(Feature::XmlAttr)
            }
            _ => None,
        })
    }
}

impl FeaturesExt for Option<Vec<Feature>> {
    fn pop_by(&mut self, op: impl FnMut(&Feature) -> bool) -> Option<Feature> {
        self.as_mut().and_then(|features| features.pop_by(op))
    }

    fn pop_value_type_feature(&mut self) -> Option<ValueType> {
        self.as_mut().and_then(|features| features.pop_value_type_feature())
    }

    fn pop_rename_feature(&mut self) -> Option<Rename> {
        self.as_mut().and_then(|features| features.pop_rename_feature())
    }

    fn pop_rename_all_feature(&mut self) -> Option<RenameAll> {
        self.as_mut().and_then(|features| features.pop_rename_all_feature())
    }

    fn extract_vec_xml_feature(&mut self, type_tree: &TypeTree) -> Option<Feature> {
        self.as_mut()
            .and_then(|features| features.extract_vec_xml_feature(type_tree))
    }
}