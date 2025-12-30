use proc_macro::TokenStream;
use quote::quote;
use syn::Data;
use syn::DeriveInput;
use syn::Fields;
use syn::parse_macro_input;

#[proc_macro_derive(OnlineConfig)]
pub fn online_config_derive(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);
	let name = input.ident;

	let fields = match input.data {
		Data::Struct(ref data) => match data.fields {
			Fields::Named(ref fields) => &fields.named,
			_ => panic!("OnlineConfig only supports structs with named fields"),
		},
		_ => panic!("OnlineConfig only supports structs"),
	};

	let match_arms = fields.iter().map(|f| {
		let field_name = &f.ident;
		let field_type = &f.ty;
		let field_name_str = field_name.as_ref().unwrap().to_string();

		quote! {
			#field_name_str => {
				match #field_type::from_str(value) {
					Ok(v) => {
						self.#field_name = v;
						Ok(())
					}
					Err(_) => Err(format!("Failed to parse value for field '{}'", #field_name_str)),
				}
			}
		}
	});

	let expanded = quote! {
		impl #name {
			pub fn set_field(&mut self, key: &str, value: &str) -> Result<(), String> {
				match key {
					#(#match_arms)*
					_ => Err(format!("Field '{}' not found", key)),
				}
			}
		}
	};

	TokenStream::from(expanded)
}
