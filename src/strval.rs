use std::{error::Error, fmt::Display};

use stellar_contract_env_host::{
    xdr::{Error as XDRError, ScObject, ScMap, ScMapEntry, ScVal, ScVec, ScStatic, ScSpecTypeDef, ScSpecTypeVec, ScSpecTypeMap},
    Host,
};

#[derive(Debug)]
pub enum StrValError {
    UnknownError,
    UnknownType,
    InvalidValue,
    XDR(XDRError),
    Serde(serde_json::Error),
}

impl Error for StrValError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

impl Display for StrValError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error: ")?;
        match self {
            Self::UnknownError => write!(f, "an unknown error occurred")?,
            Self::UnknownType => write!(f, "unknown type specified")?,
            Self::InvalidValue => write!(f, "value is not parseable to type")?,
            Self::Serde(_) => write!(f, "json error")?,
            Self::XDR(_) => write!(f, "xdr error")?,
        };
        Ok(())
    }
}

impl From<std::num::ParseIntError> for StrValError {
    fn from(_: std::num::ParseIntError) -> Self {
        StrValError::InvalidValue
    }
}

impl From<()> for StrValError {
    fn from(_: ()) -> Self {
        StrValError::UnknownError
    }
}

pub fn from_string(s: &str, t: &ScSpecTypeDef) -> Result<ScVal, StrValError> {
    let val: ScVal = match t {
        // These ones have special processing when they're the top-level args. This is so we don't
        // need extra quotes around string args.
        ScSpecTypeDef::Symbol => ScVal::Symbol(s.as_bytes().try_into().map_err(|_| StrValError::InvalidValue)?),
        ScSpecTypeDef::Binary => ScVal::Object(Some(ScObject::Binary(s.as_bytes().try_into().map_err(|_| StrValError::InvalidValue)?))),

        // For all others we just use the json parser
        _ => serde_json::from_str(s).map_err(StrValError::Serde).and_then(|raw| from_json(&raw, t))?,

    };
    Ok(val)
}

pub fn from_json(v: &serde_json::Value, t: &ScSpecTypeDef) -> Result<ScVal, StrValError> {
    let val: ScVal = match (t, v) {
        // Boolean parsing
        (ScSpecTypeDef::Bool, serde_json::Value::Bool(true)) =>
            ScVal::Static(ScStatic::True),
        (ScSpecTypeDef::Bool, serde_json::Value::Bool(false)) =>
            ScVal::Static(ScStatic::False),

        // Vec parsing
        (ScSpecTypeDef::Vec(elem), serde_json::Value::Array(raw)) => {
            let ScSpecTypeVec{ element_type } = *elem.to_owned();
            let parsed: Result<Vec<ScVal>, StrValError> = raw.iter().map(|item| -> Result<ScVal, StrValError> {
                from_json(item, &element_type)
            }).collect();
            let converted : ScVec = parsed?.try_into().map_err(StrValError::XDR).unwrap();
            ScVal::Object(Some(ScObject::Vec(converted)))
        },

        // Number parsing
        (ScSpecTypeDef::BigInt, serde_json::Value::String(_n)) =>
            // TODO: Implement this
            return Err(StrValError::InvalidValue),
        (ScSpecTypeDef::BigInt, serde_json::Value::Number(_n)) =>
            // TODO: Implement this
            return Err(StrValError::InvalidValue),
        (ScSpecTypeDef::I32, serde_json::Value::Number(n)) =>
            {
            ScVal::I32(
                n.as_i64().
                    ok_or(StrValError::InvalidValue)?.
                    try_into().
                    map_err(|_| StrValError::InvalidValue)?
            )
        },
        (ScSpecTypeDef::I64, serde_json::Value::Number(n)) =>
            ScVal::Object(Some(ScObject::I64(n.as_i64().ok_or(StrValError::InvalidValue)?))),
        (ScSpecTypeDef::U32, serde_json::Value::Number(n)) => {
            ScVal::U32(
                n.as_u64().
                    ok_or(StrValError::InvalidValue)?.
                    try_into().
                    map_err(|_| StrValError::InvalidValue)?
            )
        },
        (ScSpecTypeDef::U64, serde_json::Value::Number(n)) =>
            ScVal::U63(n.as_i64().ok_or(StrValError::InvalidValue)?),

        // Map parsing
        (ScSpecTypeDef::Map(map), serde_json::Value::Object(raw)) => {
            let ScSpecTypeMap{key_type, value_type} = *map.to_owned();
            // TODO: What do we do if the expected key_type is not a string or symbol?
            let parsed: Result<Vec<ScMapEntry>, StrValError> = raw.iter().map(|(k, v)| -> Result<ScMapEntry, StrValError> {
                let key = from_string(k, &key_type)?;
                let val = from_json(v, &value_type)?;
                Ok(ScMapEntry{key, val})
            }).collect();
            let converted : ScMap = parsed?.try_into().map_err(StrValError::XDR).unwrap();
            ScVal::Object(Some(ScObject::Map(converted)))
        },

        // Symbol & String parsing
        (ScSpecTypeDef::Symbol, serde_json::Value::String(s)) =>
            ScVal::Symbol(s.as_bytes().try_into().map_err(|_| StrValError::InvalidValue)?),

        // Binary parsing
        (ScSpecTypeDef::Binary, serde_json::Value::String(s)) =>
            ScVal::Object(Some(ScObject::Binary(s.as_bytes().try_into().map_err(|_| StrValError::InvalidValue)?))),
        (ScSpecTypeDef::Binary, serde_json::Value::Array(_raw)) => {
            return Err(StrValError::InvalidValue); // TODO: Implement this
            // let b: Result<Vec<u8>, StrValError> = raw.iter().map(|item| item.as_u64().try_into().map_err(|_| StrValError::InvalidValue)).collect();
            // ScVal::Object(Some(ScObject::Binary(b?)))
        },

        // Option parsing
        (ScSpecTypeDef::Option(_), serde_json::Value::Null) =>
            // is null -> void the right thing here?
            ScVal::Object(None),
        (ScSpecTypeDef::Option(_elem), _v) => {
            return Err(StrValError::InvalidValue); // TODO: Implement this
            // let ScSpecTypeOption{ value_type } = *elem.to_owned();
            // ScVal::Object(Some(from_json(v, &value_type)?.try_into()?))
        },

        // TODO: Implement the rest of these
        // ScSpecTypeDef::Bitset => {},
        // ScSpecTypeDef::Status => {},
        // ScSpecTypeDef::BigInt => ScVal::Object(Some(ScObject::BigInt(s.parse()?))),
        // ScSpecTypeDef::Result(Box<ScSpecTypeResult>) => {},
        // ScSpecTypeDef::Set(Box<ScSpecTypeSet>) => {},
        // ScSpecTypeDef::Tuple(Box<ScSpecTypeTuple>) => {},
        // ScSpecTypeDef::Udt(ScSpecTypeUdt) => {},
        _ => return Err(StrValError::UnknownType),
    };
    Ok(val)
}

pub fn to_string(_h: &Host, v: ScVal) -> String {
    #[allow(clippy::match_same_arms)]
    match v {
        ScVal::I32(v) => format!("{}", v),
        ScVal::U32(v) => format!("{}", v),
        ScVal::U63(v) => format!("{}", v),
        ScVal::Static(v) => match v {
            ScStatic::True => "true",
            ScStatic::False => "false",
            ScStatic::Void => "void",
            _ => "todo!"
        }.to_string(),
        ScVal::Symbol(v) => format!(
            "{}",
            std::str::from_utf8(v.as_slice()).expect("non-UTF-8 in symbol")
        ),
        ScVal::Bitset(_) => todo!(),
        ScVal::Status(_) => todo!(),
        ScVal::Object(None) => panic!(""),
        ScVal::Object(Some(b)) => match b {
            ScObject::Vec(_) => todo!(),
            ScObject::Map(_) => todo!(),
            ScObject::U64(v) => format!("{}", v),
            ScObject::I64(v) => format!("{}", v),
            ScObject::Binary(_) => todo!(),
            ScObject::BigInt(_) => todo!(),
            ScObject::Hash(_) => todo!(),
            ScObject::PublicKey(_) => todo!(),
        },
    }
}
