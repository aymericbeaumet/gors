use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub(super) struct MethodSet {
    pub(super) direct_methods: Vec<String>,
    pub(super) required_methods: Vec<String>,
    pub(super) embedded_interfaces: Vec<String>,
}

pub(super) fn for_impl(trait_name: &str, fallback_required_methods: &[String]) -> MethodSet {
    let direct_methods = direct_methods_for_impl(trait_name, fallback_required_methods);
    let required_methods = if trait_name.contains('.') {
        methods_from_import(trait_name)
            .or_else(|| super::TYPE_ENV.with(|env| env.borrow().get_interface_methods(trait_name)))
    } else {
        super::TYPE_ENV
            .with(|env| env.borrow().get_interface_methods(trait_name))
            .or_else(|| methods_from_import(trait_name))
    }
    .unwrap_or_else(|| fallback_required_methods.to_vec());
    let embedded_interfaces = embedded_interfaces_for_impl(trait_name);

    MethodSet {
        direct_methods,
        required_methods,
        embedded_interfaces,
    }
}

pub(super) fn pointer_satisfies(
    struct_method_list: &[String],
    required_methods: &[String],
) -> bool {
    required_methods
        .iter()
        .all(|method| struct_method_list.contains(method))
}

pub(super) fn value_type_satisfies(
    struct_name: &str,
    trait_name: &str,
    struct_method_list: &[String],
    pointer_methods: Option<&BTreeSet<String>>,
    required_methods: &[String],
) -> bool {
    super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        if env.is_interface(trait_name) {
            env.named_type_implements_interface(struct_name, trait_name, false)
        } else {
            value_method_list_satisfies(struct_method_list, pointer_methods, required_methods)
        }
    })
}

fn direct_methods_for_impl(trait_name: &str, fallback: &[String]) -> Vec<String> {
    super::TYPE_ENV.with(|env| {
        env.borrow()
            .get_interface_direct_methods(trait_name)
            .or_else(|| direct_methods_from_import(trait_name))
            .unwrap_or_else(|| fallback.to_vec())
    })
}

fn direct_methods_from_import(trait_name: &str) -> Option<Vec<String>> {
    let (package_name, type_name) = trait_name.split_once('.')?;
    let (_, env) = crate::resolve::scan_type_env(package_name)?;
    env.get_interface_direct_methods(type_name)
}

fn methods_from_import(trait_name: &str) -> Option<Vec<String>> {
    let mut visiting = BTreeSet::new();
    methods_from_import_inner(trait_name, &mut visiting)
}

fn methods_from_import_inner(
    trait_name: &str,
    visiting: &mut BTreeSet<String>,
) -> Option<Vec<String>> {
    if !visiting.insert(trait_name.to_string()) {
        return Some(Vec::new());
    }
    let (package_name, type_name) = trait_name.split_once('.')?;
    let (_, env) = crate::resolve::scan_type_env(package_name)?;
    let mut methods = env.get_interface_direct_methods(type_name)?;
    for embedded_name in env.get_interface_direct_embedded_interfaces(type_name) {
        let embedded_name = qualify_import_interface_name(package_name, &embedded_name);
        let Some(embedded_methods) = methods_from_import_inner(&embedded_name, visiting) else {
            continue;
        };
        for method in embedded_methods {
            if !methods.contains(&method) {
                methods.push(method);
            }
        }
    }
    visiting.remove(trait_name);
    Some(methods)
}

fn embedded_interfaces_for_impl(trait_name: &str) -> Vec<String> {
    if trait_name.contains('.') {
        let imported = embedded_interfaces_from_import(trait_name).unwrap_or_default();
        if !imported.is_empty() {
            return imported;
        }
    }
    let embedded =
        super::TYPE_ENV.with(|env| env.borrow().get_interface_embedded_interfaces(trait_name));
    if !embedded.is_empty() {
        return embedded;
    }
    embedded_interfaces_from_import(trait_name).unwrap_or_default()
}

fn embedded_interfaces_from_import(trait_name: &str) -> Option<Vec<String>> {
    let mut visiting = BTreeSet::new();
    let mut out = Vec::new();
    collect_embedded_interfaces_from_import(trait_name, &mut visiting, &mut out)?;
    Some(out)
}

fn collect_embedded_interfaces_from_import(
    trait_name: &str,
    visiting: &mut BTreeSet<String>,
    out: &mut Vec<String>,
) -> Option<()> {
    if !visiting.insert(trait_name.to_string()) {
        return Some(());
    }
    let (package_name, type_name) = trait_name.split_once('.')?;
    let (_, env) = crate::resolve::scan_type_env(package_name)?;
    for embedded_name in env.get_interface_direct_embedded_interfaces(type_name) {
        let embedded_name = qualify_import_interface_name(package_name, &embedded_name);
        if !out.contains(&embedded_name) {
            out.push(embedded_name.clone());
        }
        collect_embedded_interfaces_from_import(&embedded_name, visiting, out)?;
    }
    visiting.remove(trait_name);
    Some(())
}

fn qualify_import_interface_name(package_name: &str, embedded_name: &str) -> String {
    if embedded_name.contains('.') {
        embedded_name.to_string()
    } else {
        format!("{package_name}.{embedded_name}")
    }
}

fn value_method_list_satisfies(
    struct_method_list: &[String],
    pointer_methods: Option<&BTreeSet<String>>,
    required_methods: &[String],
) -> bool {
    required_methods.iter().all(|method| {
        struct_method_list.contains(method)
            && pointer_methods.is_none_or(|methods| !methods.contains(method))
    })
}
