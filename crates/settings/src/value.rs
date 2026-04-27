use gpui::SharedString;

#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    String(SharedString),
    Int(i64),
    List(Vec<SettingValue>),
}

impl SettingValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_ref()),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[SettingValue]> {
        match self {
            Self::List(v) => Some(v),
            _ => None,
        }
    }
}
