pub mod app;
pub mod config;
pub mod dir;
pub mod external_event;
pub mod queue;
pub mod ui;

macro_rules! switch {
    ($input:expr; $first:expr$(, $($conditions:expr),*)? => $execute:expr, $($rest:expr$(, $($conditions_rest:expr),*)? => $exec:expr),*$(, _ => $execute_last:expr)?$(,)?) => {
        if $first == $input $(&& $($conditions)&&*)? { $execute }
        $(else if $rest == $input $(&& $($conditions_rest)&&*)? { $exec })*
        $(else { $execute_last })?
    };
}
pub(crate) use switch;
