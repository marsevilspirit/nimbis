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

	// Generate field names array for list_fields
	let field_names: Vec<_> = fields
		.iter()
		.map(|f| {
			let field_name_str = f.ident.as_ref().unwrap().to_string();
			quote! { #field_name_str }
		})
		.collect();

	// Generate match arms for getting all fields
	let all_fields_pairs = fields.iter().map(|f| {
		let field_name = &f.ident;
		let field_name_str = field_name.as_ref().unwrap().to_string();

		quote! {
			(#field_name_str.to_string(), self.#field_name.to_string())
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

			/// List all available field names
			pub fn list_fields() -> Vec<&'static str> {
				vec![#(#field_names),*]
			}

			/// Get all fields as key-value pairs
			pub fn get_all_fields(&self) -> Vec<(String, String)> {
				vec![#(#all_fields_pairs),*]
			}

			/// Match fields by wildcard pattern
			/// Supports:
			/// - "*" for all fields
			/// - "prefix*" for prefix matching
			/// - "*suffix" for suffix matching
			pub fn match_fields(pattern: &str) -> Vec<&'static str> {
				let all_fields = Self::list_fields();

				if pattern == "*" {
					return all_fields;
				}

				if let Some(stripped) = pattern.strip_prefix('*') {
					if let Some(middle) = stripped.strip_suffix('*') {
						// Contains match: *middle*
						all_fields.into_iter()
							.filter(|field| field.contains(middle))
							.collect()
					} else {
						// Suffix match: *suffix
						all_fields.into_iter()
							.filter(|field| field.ends_with(stripped))
							.collect()
					}
				} else if let Some(prefix) = pattern.strip_suffix('*') {
					// Prefix match: prefix*
					all_fields.into_iter()
						.filter(|field| field.starts_with(prefix))
						.collect()
				} else {
					// Exact match
					all_fields.into_iter()
						.filter(|field| *field == pattern)
						.collect()
				}
			}
		}
	};

	TokenStream::from(expanded)
}
