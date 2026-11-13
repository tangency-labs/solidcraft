//! Typed records for dialogs whose defaults and scalar restoration are mechanical.
//! Selection, geometry, optional parameters and form rows stay with their callers.

macro_rules! dialog_descriptors {
    (
        $(#[$attr:meta])* $vis:vis enum $name:ident {
            $( $variant:ident ($command:literal, $title:literal,
                [$(($label:literal, $accept:expr, $many:expr)),* $(,)?], $feature:pat) {
                $( $field:ident: $ty:ty = $default:expr => $restore:expr, )*
            } => $params:expr; )*
            ; custom { $($custom:tt)* }
        }
    ) => {
        $(#[$attr])* $vis enum $name {
            $( $variant { $($field: $ty,)* }, )*
            $($custom)*
        }

        impl $name {
            pub(crate) fn simple_start(id: &str) -> Option<(Self, Vec<crate::selection::SelInput>)> {
                let kind = match id {
                    $( $command => Self::$variant { $($field: $default,)* }, )*
                    _ => return None,
                };
                let inputs = kind.simple_inputs();
                Some((kind, inputs))
            }

            pub(crate) fn simple_inputs(&self) -> Vec<crate::selection::SelInput> {
                match self {
                    $( Self::$variant { .. } => vec![$(crate::selection::SelInput::new($label, $accept, $many)),*], )*
                    _ => Vec::new(),
                }
            }

            pub(crate) fn simple_metadata(&self) -> Option<(&'static str, &'static str)> {
                match self {
                    $( Self::$variant { .. } => Some(($command, $title)), )*
                    _ => None,
                }
            }

            pub(crate) fn fill_simple_params(&self, params: &mut serde_json::Value) {
                let scalars = match self {
                    $( Self::$variant { $($field,)* } => {
                        // Some fields have conditional emission in the explicit command builder.
                        let _ = ($($field,)*);
                        $params
                    }, )*
                    _ => return,
                };
                if let (serde_json::Value::Object(params), serde_json::Value::Object(scalars)) = (params, scalars) {
                    for (key, value) in scalars {
                        params.entry(key).or_insert(value);
                    }
                }
            }

            pub(crate) fn simple_restore(feature: &solidcraft_engine::doc::FeatureKind) -> Option<Self> {
                Some(match feature {
                    $( $feature => Self::$variant { $($field: $restore,)* }, )*
                    _ => return None,
                })
            }
        }
    };
}

pub(crate) use dialog_descriptors;
