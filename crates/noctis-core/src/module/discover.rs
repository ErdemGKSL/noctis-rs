//! Discovered registrations (FR-81, AD-31): structs whose kind attribute
//! names their Module (`module = "a/b"`) submit a [`Discovered`] entry at
//! link time with `inventory`; `AppBuilder::discover()` merges every linked
//! entry into the mounted Module trees before pruning and the fold, as if
//! each had been written with `Module::add`.

use std::collections::BTreeSet;

use super::Module;
use crate::BuildViolation;

/// One discovered registration: the Module id its kind attribute names, the
/// struct's type path and its `Register::__register`. Only the derives build
/// it, through `inventory::submit!`.
#[doc(hidden)]
#[derive(Debug)]
pub struct Discovered {
    module: &'static str,
    name: &'static str,
    register: fn(Module) -> Module,
}

impl Discovered {
    /// The entry of the struct `name` (its `__Kind::NAME`) for the Module
    /// `module`; what the derives submit.
    #[doc(hidden)]
    #[must_use]
    pub const fn __new(
        module: &'static str,
        name: &'static str,
        register: fn(Module) -> Module,
    ) -> Self {
        Self {
            module,
            name,
            register,
        }
    }
}

inventory::collect!(Discovered);

/// One discovered Module declaration: the `fn() -> Module` that
/// `#[noctis::module]` marks. Only that attribute builds it, through
/// `inventory::submit!`.
#[doc(hidden)]
#[derive(Debug)]
pub struct DiscoveredModule {
    build: fn() -> Module,
}

impl DiscoveredModule {
    /// The entry of the declaring fn `build`; what `#[noctis::module]`
    /// submits.
    #[doc(hidden)]
    #[must_use]
    pub const fn __new(build: fn() -> Module) -> Self {
        Self { build }
    }
}

inventory::collect!(DiscoveredModule);

/// Every linked Module declaration, built, sorted by id (a parent path
/// sorts before its children).
pub(crate) fn linked_modules() -> Vec<Module> {
    let mut modules: Vec<Module> = inventory::iter::<DiscoveredModule>()
        .map(|d| (d.build)())
        .collect();
    modules.sort_by_key(Module::id);
    modules
}

/// Mounts the declared `modules` (in order) into `trees`: a root id as a
/// new tree, a nested id as the last child of the Module of its parent path
/// (found in any tree, or created as in [`mount`]). An id mounted twice is
/// left to `build()`'s `DuplicateModule` check.
pub(crate) fn mount_modules(trees: &mut Vec<Module>, modules: Vec<Module>) {
    for module in modules {
        match module.id.rfind('/') {
            Some(split) => {
                let parent = &module.id[..split];
                let path = ensure(trees, parent);
                at_mut(trees, &path).children.push(module);
            }
            None => trees.push(module),
        }
    }
}

/// Every linked entry, sorted by Module id, then type path, so link and
/// file order never change the result.
pub(crate) fn linked() -> Vec<&'static Discovered> {
    let mut entries: Vec<&'static Discovered> = inventory::iter::<Discovered>().collect();
    entries.sort_unstable_by_key(|e| (e.module, e.name));
    entries
}

/// The number of linked registrations and Module declarations (for the
/// `debug` line of an App that does not discover).
pub(crate) fn linked_count() -> usize {
    inventory::iter::<Discovered>().count() + inventory::iter::<DiscoveredModule>().count()
}

/// Merges `entries` (in order) into `trees`: each into the Module with its
/// id anywhere in any tree (after that Module's own registrations), else
/// into a new `Module::new(id)` nested under the Module of its parent path,
/// created the same way, or pushed as a new root. A struct some Module
/// already registered with `Module::add` is not merged again; it is a
/// [`BuildViolation::DuplicateRegistration`] in `violations`.
pub(crate) fn mount(
    trees: &mut Vec<Module>,
    entries: &[&Discovered],
    violations: &mut Vec<BuildViolation>,
) {
    let mut added = BTreeSet::new();
    for tree in trees.iter() {
        collect_added(tree, &mut added);
    }
    for entry in entries {
        if added.contains(entry.name) {
            violations.push(BuildViolation::DuplicateRegistration {
                type_name: entry.name,
                module: entry.module,
            });
            continue;
        }
        let path = ensure(trees, entry.module);
        let target = at_mut(trees, &path);
        let module = std::mem::replace(target, Module::new(entry.module));
        *target = (entry.register)(module);
    }
}

/// Adds the `Module::add` type paths of `module`'s whole tree to `out`.
fn collect_added(module: &Module, out: &mut BTreeSet<&'static str>) {
    out.extend(module.added.iter().copied());
    for child in &module.children {
        collect_added(child, out);
    }
}

/// The index path of the first Module with id `id` (pre-order over the
/// trees, parent before children).
fn find(trees: &[Module], id: &str) -> Option<Vec<usize>> {
    for (index, module) in trees.iter().enumerate() {
        if module.id == id {
            return Some(vec![index]);
        }
        if let Some(mut rest) = find(&module.children, id) {
            rest.insert(0, index);
            return Some(rest);
        }
    }
    None
}

/// The index path of the Module `id`, creating it (and its missing
/// ancestors) when no tree holds it.
fn ensure(trees: &mut Vec<Module>, id: &'static str) -> Vec<usize> {
    if let Some(path) = find(trees, id) {
        return path;
    }
    if let Some(split) = id.rfind('/') {
        let mut path = ensure(trees, &id[..split]);
        let parent = at_mut(trees, &path);
        parent.children.push(Module::new(id));
        path.push(parent.children.len() - 1);
        path
    } else {
        trees.push(Module::new(id));
        vec![trees.len() - 1]
    }
}

/// The Module at the index path `path` (from [`find`] or [`ensure`]).
/// Both build non-empty paths of valid indexes.
fn at_mut<'a>(trees: &'a mut [Module], path: &[usize]) -> &'a mut Module {
    let (first, rest) = path.split_first().unwrap_or((&0, &[]));
    let mut module = &mut trees[*first];
    for index in rest {
        module = &mut module.children[*index];
    }
    module
}

#[cfg(test)]
mod tests;
