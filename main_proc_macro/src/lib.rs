use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let name = &input.sig.ident;

    // ensure we emit _start in the same module as the fn
    let expanded = quote! {
        #input

        #[unsafe(no_mangle)]
        pub extern "Rust" fn _start() {
            #name();
        }
    };

    expanded.into()
}
