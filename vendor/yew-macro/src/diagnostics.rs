use std::cell::RefCell;

use proc_macro2::{Span, TokenStream};

thread_local! {
    static ERRORS: RefCell<Vec<syn::Error>> = const { RefCell::new(Vec::new()) };
}

struct ErrorScope(Vec<syn::Error>);

impl Drop for ErrorScope {
    fn drop(&mut self) {
        ERRORS.with(|errors| *errors.borrow_mut() = std::mem::take(&mut self.0));
    }
}

pub(crate) fn run<T: Into<TokenStream>>(expand: impl FnOnce() -> T) -> TokenStream {
    let _scope = ErrorScope(ERRORS.with(|errors| std::mem::take(&mut *errors.borrow_mut())));
    let expanded = expand().into();
    ERRORS.with(|errors| {
        let errors = std::mem::take(&mut *errors.borrow_mut());
        if errors.is_empty() {
            expanded
        } else {
            errors
                .into_iter()
                .map(|error| error.into_compile_error())
                .collect()
        }
    })
}

pub(crate) fn error(error: syn::Error) {
    ERRORS.with(|errors| errors.borrow_mut().push(error));
}

#[rustversion::nightly]
pub(crate) fn warning(span: Span, message: impl Into<String>) {
    proc_macro::Diagnostic::spanned(span.unwrap(), proc_macro::Level::Warning, message).emit();
}

#[rustversion::not(nightly)]
pub(crate) fn warning(_span: Span, _message: impl Into<String>) {}

macro_rules! emit_error {
    ($span:expr, $message:expr; $($kind:ident = $child:expr);+ $(;)?) => {{
        let mut message = $message.to_string();
        message.push('\n');
        $(message.push_str(&format!("\n  = {}: {}", stringify!($kind), $child));)+
        message.push('\n');
        $crate::diagnostics::error(syn::Error::new_spanned(
            &$span,
            message,
        ))
    }};
    ($span:expr, $message:expr) => {
        $crate::diagnostics::error(syn::Error::new_spanned(&$span, $message))
    };
}

macro_rules! emit_warning {
    ($span:expr, $message:expr) => {
        $crate::diagnostics::warning($span, $message)
    };
}

pub(crate) use {emit_error, emit_warning};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_expansion_is_unchanged() {
        let original = quote::quote!(
            fn example() {}
        );
        assert_eq!(run(|| original.clone()).to_string(), original.to_string());
    }

    #[test]
    fn errors_replace_expansion_and_do_not_leak() {
        let result = run(|| {
            error(syn::Error::new(Span::call_site(), "first"));
            error(syn::Error::new(Span::call_site(), "second"));
            quote::quote!(
                fn invalid() {}
            )
        });
        let tokens = result.to_string();
        assert!(tokens.contains("first"));
        assert!(tokens.contains("second"));
        assert!(!tokens.contains("invalid"));
        assert!(run(TokenStream::new).is_empty());
    }

    #[test]
    fn panic_does_not_leak_errors() {
        let result = std::panic::catch_unwind(|| {
            run::<TokenStream>(|| {
                error(syn::Error::new(Span::call_site(), "before panic"));
                panic!("test panic")
            })
        });
        assert!(result.is_err());
        assert!(run(TokenStream::new).is_empty());
    }
}
