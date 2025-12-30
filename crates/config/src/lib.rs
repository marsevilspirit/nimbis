use proc_macro::TokenStream;
use quote::quote;
use syn::Data;
use syn::DeriveInput;
use syn::Fields;
use syn::parse_macro_input;

#[proc_macro_derive(OnlineConfig, attributes(online_config))]
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

	let set_match_arms = fields.iter().map(|f| {
		let field_name = &f.ident;
		let field_type = &f.ty;
		let field_name_str = field_name.as_ref().unwrap().to_string();

		let mut is_immutable = false;
		for attr in &f.attrs {
			if attr.path().is_ident("online_config") {
				let _ = attr.parse_nested_meta(|meta| {
					if meta.path.is_ident("immutable") {
						is_immutable = true;
					}
					Ok(())
				});
			}
		}

		if is_immutable {
			quote! {
				#field_name_str => {
					Err(format!("Field '{}' is immutable", #field_name_str))
				}
			}
		} else {
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
		}
	});

	// Generate match arms for get_field
	let get_match_arms = fields.iter().map(|f| {
		let field_name = &f.ident;
		let field_name_str = field_name.as_ref().unwrap().to_string();

		quote! {
			#field_name_str => Ok(self.#field_name.to_string()),
		}
	});

	let expanded = quote! {
		impl #name {
			pub fn set_field(&mut self, key: &str, value: &str) -> Result<(), String> {
				match key {
					#(#set_match_arms)*
					_ => Err(format!("Field '{}' not found", key)),
				}
			}

			pub fn get_field(&self, key: &str) -> Result<String, String> {
				match key {
					#(#get_match_arms)*
					_ => Err(format!("Field '{}' not found", key)),
				}
			}
		}
	};

	TokenStream::from(expanded)
}
