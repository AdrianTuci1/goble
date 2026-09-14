//! The settings view: the navigation rail and one pane per settings page.
//!
//! One module per surface of the view: the builder chain that feeds it state
//! ([`builder`]), the navigation rail ([`nav`]), the row every setting is built
//! from ([`row`]), the pages themselves ([`pages`]) and the element protocol
//! the view answers ([`element`]). What the surfaces share — the view's own
//! state and its page enum — lives here.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{Element, Point};
use crate::geometry::Vector2F;

mod builder;
mod element;
mod nav;
mod pages;
mod row;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    Profile,
    Llm,
    Appearance,
    Account,
    Cluster,
    Workers,
    Keys,
}

pub struct SettingsView {
    current_page: SettingsPage,
    profile_name: String,
    profile_email: String,
    llm_provider: String,
    llm_model: String,
    llm_api_key: String,
    llm_base_url: String,
    llm_temperature: String,
    dark_mode: bool,
    vault_unlocked: bool,
    vault_secrets: Vec<String>,
    cluster_name: String,
    cluster_configured: bool,
    workers: Vec<(String, String, String, bool)>, // id, name, url, paired
    authorized_keys: Vec<(String, String, String)>, // id, name, fingerprint
    on_navigate: Option<Rc<RefCell<dyn FnMut(SettingsPage) + 'static>>>,
    on_save_profile: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_save_llm: Option<Rc<RefCell<dyn FnMut(String, String, String, String, String) + 'static>>>,
    on_toggle_dark_mode: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_unlock_vault: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_add_vault_secret: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_create_cluster: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_unlock_cluster: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_add_worker: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_pair_worker: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_remove_worker: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_add_authorized_key: Option<Rc<RefCell<dyn FnMut(String, String, String) + 'static>>>,
    on_remove_authorized_key: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_back: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsView {
    pub fn new(current_page: SettingsPage) -> Self {
        Self {
            current_page,
            profile_name: String::new(),
            profile_email: String::new(),
            llm_provider: String::new(),
            llm_model: String::new(),
            llm_api_key: String::new(),
            llm_base_url: String::new(),
            llm_temperature: String::new(),
            dark_mode: false,
            vault_unlocked: false,
            vault_secrets: Vec::new(),
            cluster_name: String::new(),
            cluster_configured: false,
            workers: Vec::new(),
            authorized_keys: Vec::new(),
            on_navigate: None,
            on_save_profile: None,
            on_save_llm: None,
            on_toggle_dark_mode: None,
            on_unlock_vault: None,
            on_add_vault_secret: None,
            on_create_cluster: None,
            on_unlock_cluster: None,
            on_add_worker: None,
            on_pair_worker: None,
            on_remove_worker: None,
            on_add_authorized_key: None,
            on_remove_authorized_key: None,
            on_back: None,
            root: None,
            size: None,
            origin: None,
        }
    }
}
