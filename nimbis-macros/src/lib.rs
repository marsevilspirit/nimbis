use proc_macro::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::Data;
use syn::DeriveInput;
use syn::Error;
use syn::Fields;
use syn::FnArg;
use syn::Ident;
use syn::ItemFn;
use syn::Result;
use syn::Token;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::parse_macro_input;

struct StorageLockArgs {
	mode: Ident,
	key: Option<Ident>,
}

impl Parse for StorageLockArgs {
	fn parse(input: ParseStream<'_>) -> Result<Self> {
		let mode = input.parse()?;
		let key = if input.peek(Token![,]) {
			input.parse::<Token![,]>()?;
			Some(input.parse()?)
		} else {
			None
		};

		if !input.is_empty() {
			return Err(input.error(
				"unsupported storage_lock arguments; expected `read, key`, `write, key`, `read_many, keys`, `write_many, keys`, or `global_write`",
			));
		}

		Ok(Self { mode, key })
	}
}

#[proc_macro_attribute]
pub fn storage_lock(attr: TokenStream, item: TokenStream) -> TokenStream {
	let args = parse_macro_input!(attr as StorageLockArgs);
	let input = parse_macro_input!(item as ItemFn);

	if input.sig.asyncness.is_none() {
		return Error::new_spanned(input.sig.fn_token, "storage_lock only supports async fn")
			.to_compile_error()
			.into();
	}

	if !matches!(input.sig.inputs.first(), Some(FnArg::Receiver(_))) {
		return Error::new_spanned(input.sig.ident, "storage_lock requires a method with self")
			.to_compile_error()
			.into();
	}

	let mode = args.mode.to_string();
	let lock = match (mode.as_str(), args.key) {
		("read", Some(key)) => quote! {
			let _guard = self.read_lock([#key.clone()]).await;
		},
		("write", Some(key)) => quote! {
			let _guard = self.write_lock([#key.clone()]).await;
		},
		("read_many", Some(keys)) => quote! {
			let #keys: Vec<_> = #keys.into_iter().collect();
			let _guard = self.read_lock(#keys.clone()).await;
		},
		("write_many", Some(keys)) => quote! {
			let #keys: Vec<_> = #keys.into_iter().collect();
			let _guard = self.write_lock(#keys.clone()).await;
		},
		("global_write", None) => quote! {
			let _guard = self.global_write_lock().await;
		},
		("global_write", Some(key)) => {
			return Error::new_spanned(key, "global_write storage_lock does not take a key")
				.to_compile_error()
				.into();
		}
		("read" | "write" | "read_many" | "write_many", None) => {
			return Error::new_spanned(args.mode, "storage_lock mode requires a key argument")
				.to_compile_error()
				.into();
		}
		_ => {
			return Error::new_spanned(
				args.mode,
				"unsupported storage_lock mode; expected `read`, `write`, `read_many`, `write_many`, or `global_write`",
			)
			.to_compile_error()
			.into();
		}
	};

	let attrs = input.attrs;
	let vis = input.vis;
	let sig = input.sig;
	let block = input.block;

	TokenStream::from(quote! {
		#(#attrs)*
		#vis #sig {
			#lock
			#block
		}
	})
}

#[proc_macro_derive(OnlineConfig, attributes(online_config))]
pub fn online_config_derive(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);
	let name = input.ident;

	let fields = match input.data {
		Data::Struct(ref data) => match data.fields {
			Fields::Named(ref fields) => &fields.named,
			_ => {
				return Error::new_spanned(
					name,
					"OnlineConfig only supports structs with named fields",
				)
				.to_compile_error()
				.into();
			}
		},
		_ => {
			return Error::new_spanned(name, "OnlineConfig only supports structs")
				.to_compile_error()
				.into();
		}
	};

	let mut parse_errors: Vec<syn::Error> = Vec::new();

	let set_match_arms: Vec<_> = fields
		.iter()
		.map(|f| {
			let field_name = &f.ident;
			let field_type = &f.ty;
			let field_name_str = field_name.as_ref().unwrap().to_string();

			let mut is_immutable = false;
			let mut callback = None;

			for attr in &f.attrs {
				if attr.path().is_ident("online_config")
					&& let Err(e) = attr.parse_nested_meta(|meta| {
						if meta.path.is_ident("immutable") {
							is_immutable = true;
							Ok(())
						} else if meta.path.is_ident("callback") {
							let value = meta.value()?;
							let s: syn::LitStr = value.parse()?;
							callback = Some(s.value());
							Ok(())
						} else {
							Err(meta.error(
								"unsupported attribute for online_config; expected `immutable` or `callback`",
							))
						}
					}) {
					parse_errors.push(e);
				}
			}

			if is_immutable {
				quote! {
					#field_name_str => {
						Err(format!("Field '{}' is immutable", #field_name_str))
					}
				}
			} else {
				let callback_invocation = if let Some(cb) = callback {
					let cb_ident = format_ident!("{}", cb);
					quote! {
						self.#cb_ident()?;
					}
				} else {
					quote! {}
				};

				quote! {
					#field_name_str => {
						match #field_type::from_str(value) {
							Ok(v) => {
								self.#field_name = v;
								#callback_invocation
								Ok(())
							}
							Err(_) => Err(format!("Failed to parse value for field '{}'", #field_name_str)),
						}
					}
				}
			}
		})
		.collect();

	// Return early if there were any parsing errors
	if !parse_errors.is_empty() {
		let errors = parse_errors.into_iter().map(|e| e.to_compile_error());
		return quote! { #(#errors)* }.into();
	}

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
