use crate::{
    attr::{Attr, AttrValue},
    context::Context,
    derive::DeriveKind,
    r#struct::Variant,
    util,
};

// Note that we intentionally exclude some unsupported primitive types
const PRIMITIVE_TYPES: &[&str] = &["bool", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"];

pub enum FieldKind {
    Primitive {
        default: Option<AttrValue<proc_macro2::TokenStream>>,
    },

    State,
}

pub struct IndexedField<'a> {
    name: Option<syn::Ident>,
    ty: &'a syn::Type,
    index: usize,
    tag: AttrValue<u16>,
    kind: FieldKind,
}

impl<'a> IndexedField<'a> {
    pub fn parse(
        context: &Context,
        kind: &DeriveKind,
        field: &'a syn::Field,
        index: usize,
    ) -> Result<Self, ()> {
        let ty = &field.ty;
        let full_type_name = quote!(#ty).to_string();
        let is_primitive = PRIMITIVE_TYPES.contains(&&*full_type_name);

        if kind == &DeriveKind::State {
            if let syn::Visibility::Inherited = field.vis {
            } else {
                context.error(&field.vis, "expected field to be private");
            }
        }

        Self::parse_attrs(context, &field, &field.attrs, is_primitive).map(|(tag, default)| {
            let kind = if is_primitive {
                FieldKind::Primitive { default }
            } else {
                FieldKind::State
            };

            Self {
                name: field.ident.clone(),
                ty,
                index,
                tag,
                kind,
            }
        })
    }

    fn parse_attrs(
        context: &Context,
        field: &syn::Field,
        attrs: &[syn::Attribute],
        is_primitive: bool,
    ) -> Result<(AttrValue<u16>, Option<AttrValue<proc_macro2::TokenStream>>), ()> {
        let mut tag_attr = Attr::new(context, "tag");
        let mut tag_encountered = false;

        let mut default_attr = Attr::new(context, "default");

        for item in attrs
            .iter()
            .flat_map(|attr| util::get_steit_meta_items(context, attr))
            .flatten()
        {
            match &item {
                syn::NestedMeta::Meta(syn::Meta::NameValue(item)) if item.path.is_ident("tag") => {
                    tag_encountered = true;

                    if let Ok(lit) = util::get_lit_int(context, "tag", &item.lit) {
                        if let Ok(tag) = lit.base10_parse() {
                            tag_attr.set(lit, tag);
                        } else {
                            context.error(lit, format!("unable to parse #[steit(tag = {})]", lit));
                        }
                    }
                }

                syn::NestedMeta::Meta(syn::Meta::NameValue(item))
                    if item.path.is_ident("default") =>
                {
                    if !is_primitive {
                        context.error(
                            item,
                            "unexpected default value for this nested steit object",
                        );
                    }

                    if let Ok(lit) = util::get_lit_str(context, "default", &item.lit) {
                        if let Ok(default) = lit.value().parse() {
                            default_attr.set(lit, default);
                        } else {
                            context.error(
                                lit,
                                format!("unable to parse #[steit(default = {:?})]", lit.value()),
                            );
                        }
                    }
                }

                syn::NestedMeta::Meta(item) => {
                    let path = item.path();
                    let path = quote!(#path).to_string().replace(' ', "");
                    context.error(item.path(), format!("unknown steit attribute `{}`", path));
                }

                syn::NestedMeta::Lit(lit) => {
                    context.error(lit, "unexpected literal in steit attributes");
                }
            }
        }

        if let Some(tag) = tag_attr.value() {
            Ok((tag, default_attr.value()))
        } else {
            if !tag_encountered {
                context.error(field, "expected a `tag` attribute #[steit(tag = ...)]");
            }

            Err(())
        }
    }

    pub fn tag(&self) -> &AttrValue<u16> {
        &self.tag
    }

    pub fn wire_type(&self) -> u8 {
        match self.kind {
            FieldKind::Primitive { .. } => 0,
            FieldKind::State => 2,
        }
    }

    pub fn get_init(&self) -> proc_macro2::TokenStream {
        let value = match &self.kind {
            FieldKind::Primitive {
                default: Some(default),
            } => {
                let default = default.get();
                quote!(#default)
            }

            FieldKind::Primitive { default: None } => quote!(Default::default()),

            FieldKind::State => {
                let ty = self.ty;
                let tag = *self.tag.get();
                quote!(<#ty>::new(runtime.nested(#tag)))
            }
        };

        get_init(&self.name, self.index, value)
    }

    pub fn get_setter(
        &self,
        struct_name: &syn::Ident,
        variant: Option<&Variant>,
    ) -> proc_macro2::TokenStream {
        let doc = format!(
            "Sets {}.",
            match &self.name {
                Some(name) => format!("`{}`", name),
                None => format!("field #{}", self.index),
            }
        );

        let ty = self.ty;
        let tag = *self.tag.get();
        let access = get_access(&self.name, self.index);

        let (name, reset, setter) = match variant {
            Some(variant) => {
                let tag = variant.tag();
                let variant = variant.ident();

                let qual = quote!(::#variant);
                let variant = util::to_snake_case(&variant.to_string());
                let new = format_ident!("new_{}", variant);

                (
                    format_ident!("set_{}_{}", variant, access.to_string()),
                    quote! {
                        if let #struct_name #qual { .. } = self {
                        } else {
                            let runtime = self.runtime().parent();
                            let value = Self::#new(runtime);
                            value.runtime().parent().log_update(#tag, &value).unwrap();
                            *self = value;
                        }
                    },
                    quote! {
                        if let #struct_name #qual { ref mut #access, .. } = self {
                            *#access = value;
                        }
                    },
                )
            }

            None => (
                format_ident!("set_{}", access.to_string()),
                quote!(),
                quote!(self.#access = value;),
            ),
        };

        match self.kind {
            FieldKind::Primitive { .. } => {
                quote! {
                    #[doc = #doc]
                    pub fn #name(&mut self, value: #ty) -> &mut Self {
                        #reset
                        self.runtime().log_update(#tag, &value).unwrap();
                        #setter
                        self
                    }
                }
            }

            FieldKind::State => {
                let name = format_ident!("{}_with", name);

                quote! {
                    #[doc = #doc]
                    pub fn #name<F: FnOnce(Runtime) -> #ty>(&mut self, get_value: F) -> &mut Self {
                        #reset
                        let runtime = self.runtime();
                        let value = get_value(runtime.nested(#tag));
                        runtime.log_update(#tag, &value).unwrap();
                        #setter
                        self
                    }
                }
            }
        }
    }

    pub fn get_sizer(&self) -> proc_macro2::TokenStream {
        let tag = *self.tag.get() as u32;
        let wire_type = self.wire_type() as u32;
        let access = get_access(&self.name, self.index);

        let sizer = match self.kind {
            FieldKind::Primitive { .. } => quote!(),
            FieldKind::State => quote!(size += self.#access.size().size();),
        };

        quote! {
            size += (#tag << 3 | #wire_type).size();
            #sizer
            size += self.#access.size();
        }
    }

    pub fn get_serializer(&self) -> proc_macro2::TokenStream {
        let tag = *self.tag.get() as u32;
        let wire_type = self.wire_type() as u32;
        let access = get_access(&self.name, self.index);

        quote! {
            (#tag << 3 | #wire_type).serialize(writer)?;
            self.#access.serialize(writer)?;
        }
    }

    pub fn get_deserializer(&self) -> proc_macro2::TokenStream {
        let tag = *self.tag.get();
        let wire_type = self.wire_type();
        let access = get_access(&self.name, self.index);

        quote!(#tag if wire_type == #wire_type => {
            self.#access.deserialize(reader)?;
        })
    }
}

pub struct RuntimeField<'a> {
    name: Option<syn::Ident>,
    ty: &'a syn::Type,
    index: usize,
}

impl<'a> RuntimeField<'a> {
    pub fn new(context: &Context, field: &'a syn::Field, index: usize) -> Self {
        if let syn::Visibility::Inherited = field.vis {
        } else {
            context.error(&field.vis, "expected `Runtime` field to be private");
        }

        Self {
            name: field.ident.clone(),
            ty: &field.ty,
            index,
        }
    }

    pub fn get_arg(&self) -> proc_macro2::TokenStream {
        let ty = self.ty;
        quote!(runtime: #ty)
    }

    pub fn get_init(&self, tag: Option<u16>) -> proc_macro2::TokenStream {
        let value = if let Some(tag) = tag {
            quote!(runtime.nested(#tag))
        } else {
            quote!(runtime)
        };

        get_init(&self.name, self.index, value)
    }

    pub fn get_access(&self) -> proc_macro2::TokenStream {
        get_access(&self.name, self.index)
    }
}

fn get_access(name: &Option<syn::Ident>, index: usize) -> proc_macro2::TokenStream {
    use quote::ToTokens;

    match name {
        Some(name) => quote!(#name),
        None => syn::Index::from(index).into_token_stream(),
    }
}

fn get_init(
    name: &Option<syn::Ident>,
    index: usize,
    value: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let access = get_access(name, index);
    quote!(#access: #value)
}
