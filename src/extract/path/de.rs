//! A small serde deserializer for path parameters.
//!
//! Supports a single value, tuples/sequences (in order) and structs/maps (by name).
//! Absent optional parameters deserialize as `None`.

use serde::de::value::Error;
use serde::de::{self, DeserializeSeed, Error as _, IntoDeserializer as _, Visitor};
use serde::forward_to_deserialize_any;

use crate::request::Param;

pub(crate) struct PathDeserializer<'de> {
    params: &'de [Param],
}

impl<'de> PathDeserializer<'de> {
    pub(crate) fn new(params: &'de [Param]) -> Self {
        Self { params }
    }

    fn single(&self) -> Result<ValueDeserializer<'de>, Error> {
        match self.params {
            [param] => Ok(ValueDeserializer::of(param)),
            _ => Err(Error::custom(format!(
                "expected 1 path parameter, the route has {}",
                self.params.len()
            ))),
        }
    }
}

macro_rules! single_value {
    ($($method:ident)*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            self.single()?.$method(visitor)
        }
    )*};
}

impl<'de> de::Deserializer<'de> for PathDeserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if self.params.len() == 1 {
            self.single()?.deserialize_any(visitor)
        } else {
            self.deserialize_map(visitor)
        }
    }

    single_value! {
        deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64
        deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char deserialize_str
        deserialize_string deserialize_bytes deserialize_byte_buf deserialize_option
        deserialize_identifier
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.single()?.deserialize_enum(name, variants, visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_seq(SeqAccess(self.params.iter()))
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        if len != self.params.len() {
            return Err(Error::custom(format!(
                "expected a tuple of {len} path parameters, the route has {}",
                self.params.len()
            )));
        }
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_map(MapAccess {
            iter: self.params.iter(),
            value: None,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_map(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }
}

struct SeqAccess<'de>(core::slice::Iter<'de, Param>);

impl<'de> de::SeqAccess<'de> for SeqAccess<'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        self.0
            .next()
            .map(|param| seed.deserialize(ValueDeserializer::of(param)))
            .transpose()
    }
}

struct MapAccess<'de> {
    iter: core::slice::Iter<'de, Param>,
    value: Option<&'de Param>,
}

impl<'de> de::MapAccess<'de> for MapAccess<'de> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some(param) = self.iter.next() else {
            return Ok(None);
        };
        self.value = Some(param);
        seed.deserialize(param.name.as_str().into_deserializer())
            .map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let param = self
            .value
            .take()
            .ok_or_else(|| Error::custom("value requested before key"))?;
        seed.deserialize(ValueDeserializer::of(param))
    }
}

/// Deserializes a single parameter, parsing primitives from their string form.
struct ValueDeserializer<'de> {
    name: &'de str,
    value: Option<&'de str>,
}

impl<'de> ValueDeserializer<'de> {
    fn of(param: &'de Param) -> Self {
        Self {
            name: &param.name,
            value: param.value.as_deref(),
        }
    }

    /// The value; only `Option`s may deserialize an absent optional parameter.
    fn present(&self) -> Result<&'de str, Error> {
        self.value.ok_or_else(|| {
            Error::custom(format!(
                "optional parameter `{}` is absent, use an Option",
                self.name
            ))
        })
    }
}

macro_rules! parse_value {
    ($($method:ident => $visit:ident,)*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            let value = self.present()?;
            match value.parse() {
                Ok(value) => visitor.$visit(value),
                Err(_) => Err(Error::custom(format!(
                    "cannot parse `{}` ({})",
                    value,
                    stringify!($method).trim_start_matches("deserialize_")
                ))),
            }
        }
    )*};
}

impl<'de> de::Deserializer<'de> for ValueDeserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Some(value) => visitor.visit_borrowed_str(value),
            None => visitor.visit_none(),
        }
    }

    parse_value! {
        deserialize_bool => visit_bool,
        deserialize_i8 => visit_i8,
        deserialize_i16 => visit_i16,
        deserialize_i32 => visit_i32,
        deserialize_i64 => visit_i64,
        deserialize_i128 => visit_i128,
        deserialize_u8 => visit_u8,
        deserialize_u16 => visit_u16,
        deserialize_u32 => visit_u32,
        deserialize_u64 => visit_u64,
        deserialize_u128 => visit_u128,
        deserialize_f32 => visit_f32,
        deserialize_f64 => visit_f64,
        deserialize_char => visit_char,
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Some(_) => visitor.visit_some(self),
            None => visitor.visit_none(),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_borrowed_str(self.present()?)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_enum(self.present()?.into_deserializer())
    }

    forward_to_deserialize_any! {
        identifier bytes byte_buf unit unit_struct seq tuple tuple_struct map struct ignored_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn params(pairs: &[(&str, &str)]) -> Vec<Param> {
        pairs
            .iter()
            .map(|(name, value)| Param {
                name: name.to_string(),
                value: Some(value.to_string()),
            })
            .collect()
    }

    #[test]
    fn single_value() {
        let p = params(&[("id", "42")]);
        assert_eq!(u32::deserialize(PathDeserializer::new(&p)).unwrap(), 42);
        assert_eq!(
            String::deserialize(PathDeserializer::new(&p)).unwrap(),
            "42"
        );
        let bad = params(&[("id", "abc")]);
        assert!(u32::deserialize(PathDeserializer::new(&bad)).is_err());
    }

    #[test]
    fn tuple_and_struct() {
        let p = params(&[("user", "7"), ("slug", "hello")]);
        let (user, slug) = <(u64, String)>::deserialize(PathDeserializer::new(&p)).unwrap();
        assert_eq!((user, slug.as_str()), (7, "hello"));
        assert!(<(u64,)>::deserialize(PathDeserializer::new(&p)).is_err());

        #[derive(Deserialize)]
        struct Params {
            slug: String,
            user: u8,
        }
        let parsed = Params::deserialize(PathDeserializer::new(&p)).unwrap();
        assert_eq!((parsed.user, parsed.slug.as_str()), (7, "hello"));
    }

    #[test]
    fn enum_value() {
        #[derive(Deserialize, Debug, PartialEq)]
        #[serde(rename_all = "lowercase")]
        enum Tab {
            Posts,
            About,
        }
        let p = params(&[("tab", "about")]);
        assert_eq!(
            Tab::deserialize(PathDeserializer::new(&p)).unwrap(),
            Tab::About
        );
    }

    #[test]
    fn absent_optional_params() {
        let p = vec![
            Param {
                name: "lang".into(),
                value: None,
            },
            Param {
                name: "slug".into(),
                value: Some("hi".into()),
            },
        ];
        let (lang, slug) =
            <(Option<String>, String)>::deserialize(PathDeserializer::new(&p)).unwrap();
        assert_eq!((lang, slug.as_str()), (None, "hi"));
        assert!(<(String, String)>::deserialize(PathDeserializer::new(&p)).is_err());

        #[derive(Deserialize)]
        struct Params {
            lang: Option<String>,
        }
        assert!(
            Params::deserialize(PathDeserializer::new(&p[..1]))
                .unwrap()
                .lang
                .is_none()
        );
    }
}
