//! The goal of this crate is to offer an easy way for bevy games to load all their assets in a loading [bevy_ecs::schedule::State].
//!
//! `bevy_asset_loader` introduces the derivable trait [AssetCollection]. Structs with asset handles
//! can be automatically loaded during a configurable loading [bevy_ecs::schedule::State]. Afterwards they will be inserted as
//! resources containing loaded handles and the plugin will switch to a second configurable [bevy_ecs::schedule::State].
//!
//! ```edition2021
//! # use bevy_asset_loader::{AssetLoader, AssetCollection};
//! # use bevy::prelude::*;
//! # use bevy::asset::AssetPlugin;
//! fn main() {
//!     let mut app = App::new();
//!     AssetLoader::new(GameState::Loading)
//!         .continue_to_state(GameState::Next)
//!         .with_collection::<AudioAssets>()
//!         .with_collection::<ImageAssets>()
//!         .build(&mut app);
//!     app
//!         .add_state(GameState::Loading)
//! # /*
//!         .add_plugins(DefaultPlugins)
//! # */
//!         .add_system_set(SystemSet::on_update(GameState::Next)
//!             .with_system(use_asset_handles.system())
//!         )
//!         # .add_plugins(MinimalPlugins)
//!         # .add_plugin(AssetPlugin::default())
//!         # .set_runner(|mut app| app.schedule.run(&mut app.world))
//!         .run();
//! }
//!
//! #[derive(AssetCollection)]
//! struct AudioAssets {
//!     #[asset(path = "audio/background.ogg")]
//!     background: Handle<AudioSource>,
//!     #[asset(path = "audio/plop.ogg")]
//!     plop: Handle<AudioSource>
//! }
//!
//! #[derive(AssetCollection)]
//! pub struct ImageAssets {
//!     #[asset(path = "images/player.png")]
//!     pub player: Handle<Image>,
//!     #[asset(path = "images/tree.png")]
//!     pub tree: Handle<Image>,
//! }
//!
//! // since this function runs in MyState::Next, we know our assets are
//! // loaded and their handles are in the resource AudioAssets
//! fn use_asset_handles(audio_assets: Res<AudioAssets>, audio: Res<Audio>) {
//!     audio.play(audio_assets.background.clone());
//! }
//!
//! #[derive(Clone, Eq, PartialEq, Debug, Hash)]
//! enum GameState {
//!     Loading,
//!     Next
//! }
//! ```

#![forbid(unsafe_code)]
#![warn(unused_imports, missing_docs)]

pub use bevy_asset_loader_derive::AssetCollection;

use bevy::app::App;
use bevy::asset::{AssetServer, HandleUntyped, LoadState};
use bevy::ecs::prelude::IntoExclusiveSystem;
use bevy::ecs::schedule::{State, StateData, SystemLabel, SystemSet};
use bevy::ecs::world::{FromWorld, World};
use bevy::utils::HashMap;
use std::marker::PhantomData;

#[cfg(feature = "progress_tracking")]
use bevy_progress_tracking::Progress;

/// Trait to mark a struct as a collection of assets
///
/// Derive is supported for structs with named fields.
/// Each field needs to be annotated with ``#[asset(path = "path/to/asset.file")]``
/// ```edition2021
/// # use bevy_asset_loader::AssetCollection;
/// # use bevy::prelude::*;
/// #[derive(AssetCollection)]
/// struct MyAssets {
///     #[asset(path = "player.png")]
///     player: Handle<Image>,
///     #[asset(path = "tree.png")]
///     tree: Handle<Image>
/// }
/// ```
pub trait AssetCollection: Send + Sync + 'static {
    /// Create a new AssetCollection from the [bevy_asset::AssetServer]
    fn create(world: &mut World) -> Self;
    /// Start loading all the assets in the collection
    fn load(world: &mut World) -> Vec<HandleUntyped>;
}

/// Extension trait for [`bevy::app::App`] enabling initialisation of [asset collections](AssetCollection)
pub trait AssetCollectionApp {
    /// Initialise an [`AssetCollection`]
    ///
    /// This function does not give any guaranties about the loading status of the asset handles.
    /// If you want to use a loading state, you do not need this function! Instead use an [`AssetLoader`]
    /// and add collections to it to be prepared during the loading state.
    fn init_collection<A: AssetCollection>(&mut self) -> &mut Self;
}

impl AssetCollectionApp for App {
    fn init_collection<Collection>(&mut self) -> &mut Self
    where
        Collection: AssetCollection,
    {
        if !self.world.contains_resource::<Collection>() {
            // This resource is required for loading a collection
            // Since bevy_asset_loader does not have a "real" Plugin,
            // we need to make sure the resource exists here
            self.init_resource::<AssetKeys>();
            // make sure the assets start to load
            let _ = Collection::load(&mut self.world);
            let resource = Collection::create(&mut self.world);
            self.insert_resource(resource);
        }
        self
    }
}

/// Extension trait for [`bevy::ecs::world::World`] enabling initialisation of [asset collections](AssetCollection)
pub trait AssetCollectionWorld {
    /// Initialise an [`AssetCollection`]
    ///
    /// This function does not give any guaranties about the loading status of the asset handles.
    /// If you want to use a loading state, you do not need this function! Instead use an [`AssetLoader`]
    /// and add collections to it to be prepared during the loading state.
    fn init_collection<A: AssetCollection>(&mut self);
}

impl AssetCollectionWorld for World {
    fn init_collection<A: AssetCollection>(&mut self) {
        if self.get_resource::<A>().is_none() {
            if self.get_resource::<AssetKeys>().is_none() {
                // This resource is required for loading a collection
                // Since bevy_asset_loader does not have a "real" Plugin,
                // we need to make sure the resource exists here
                self.insert_resource(AssetKeys::default());
            }
            // make sure the assets start to load
            let _ = A::load(self);
            let collection = A::create(self);
            self.insert_resource(collection);
        }
    }
}

struct LoadingAssetHandles<A: AssetCollection> {
    handles: Vec<HandleUntyped>,
    marker: PhantomData<A>,
}

struct AssetLoaderConfiguration<T> {
    configuration: HashMap<T, LoadingConfiguration<T>>,
}

impl<T> Default for AssetLoaderConfiguration<T> {
    fn default() -> Self {
        AssetLoaderConfiguration {
            configuration: HashMap::default(),
        }
    }
}

struct LoadingConfiguration<T> {
    next: Option<T>,
    count: usize,
}

/// Resource to dynamically resolve keys to asset paths.
///
/// This resource is set by the [AssetLoader] and is read when entering a loading state.
/// You should set your desired asset key and paths in a previous [bevy_ecs::schedule::State].
///
/// ```edition2021
/// # use bevy::prelude::*;
/// # use bevy_asset_loader::{AssetKeys, AssetCollection};
/// fn choose_character(
///     mut state: ResMut<State<GameState>>,
///     mut asset_keys: ResMut<AssetKeys>,
///     mouse_input: Res<Input<MouseButton>>,
/// ) {
///     if mouse_input.just_pressed(MouseButton::Left) {
///         asset_keys.set_asset_key("character", "images/female_adventurer.png")
///     } else if mouse_input.just_pressed(MouseButton::Right) {
///         asset_keys.set_asset_key("character", "images/zombie.png")
///     } else {
///         return;
///     }
///
///     state
///         .set(GameState::Loading)
///         .expect("Failed to change state");
/// }
///
/// #[derive(AssetCollection)]
/// struct ImageAssets {
///     #[asset(key = "character")]
///     player: Handle<Image>,
/// }
/// # #[derive(Clone, Eq, PartialEq, Debug, Hash)]
/// # enum GameState {
/// #     Loading,
/// #     Menu
/// # }
/// ```
#[derive(Default)]
pub struct AssetKeys {
    keys: HashMap<String, String>,
}

impl AssetKeys {
    /// Get the asset path corresponding to the given key.
    ///
    /// PANIC: if the key does not exist
    pub fn get_path_for_key(&self, key: &str) -> &str {
        self.keys
            .get(key)
            .unwrap_or_else(|| panic!("Failed to get a path for key '{}'", key))
    }

    /// Set the corresponding asset path for the given key.
    ///
    /// In case the key is already known, its value will be overwritten.
    /// ```edition2021
    /// # use bevy::prelude::*;
    /// # use bevy_asset_loader::{AssetKeys, AssetCollection};
    /// fn choose_character(
    ///     mut state: ResMut<State<GameState>>,
    ///     mut asset_keys: ResMut<AssetKeys>,
    ///     mouse_input: Res<Input<MouseButton>>,
    /// ) {
    ///     if mouse_input.just_pressed(MouseButton::Left) {
    ///         asset_keys.set_asset_key("character", "images/female_adventurer.png")
    ///     } else if mouse_input.just_pressed(MouseButton::Right) {
    ///         asset_keys.set_asset_key("character", "images/zombie.png")
    ///     } else {
    ///         return;
    ///     }
    ///
    ///     state
    ///         .set(GameState::Loading)
    ///         .expect("Failed to change state");
    /// }
    ///
    /// #[derive(AssetCollection)]
    /// struct ImageAssets {
    ///     #[asset(key = "character")]
    ///     player: Handle<Image>,
    /// }
    /// # #[derive(Clone, Eq, PartialEq, Debug, Hash)]
    /// # enum GameState {
    /// #     Loading,
    /// #     Menu
    /// # }
    /// ```
    pub fn set_asset_key<T: Into<String>>(&mut self, key: T, value: T) {
        self.keys.insert(key.into(), value.into());
    }
}

fn start_loading<T: StateData, Assets: AssetCollection>(world: &mut World) {
    {
        let cell = world.cell();
        let mut asset_loader_configuration = cell
            .get_resource_mut::<AssetLoaderConfiguration<T>>()
            .expect("Cannot get AssetLoaderConfiguration");
        let state = cell.get_resource::<State<T>>().expect("Cannot get state");
        let mut config = asset_loader_configuration
            .configuration
            .get_mut(state.current())
            .unwrap_or_else(|| {
                panic!(
                    "Could not find a loading configuration for state {:?}",
                    state.current()
                )
            });
        config.count += 1;
    }
    let handles = LoadingAssetHandles {
        handles: Assets::load(world),
        marker: PhantomData::<Assets>,
    };
    world.insert_resource(handles);
}

fn check_loading_state<T: StateData, Assets: AssetCollection>(world: &mut World) {
    {
        let cell = world.cell();

        let loading_asset_handles = cell.get_resource::<LoadingAssetHandles<Assets>>();
        if loading_asset_handles.is_none() {
            return;
        }
        let loading_asset_handles = loading_asset_handles.unwrap();

        let asset_server = cell
            .get_resource::<AssetServer>()
            .expect("Cannot get AssetServer resource");
        let loaded_handles = loading_asset_handles
            .handles
            .iter()
            .map(|handle| handle.id)
            .map(|handle| asset_server.get_load_state(handle))
            .filter(|state| state == &LoadState::Loaded)
            .count();

        #[cfg(feature = "progress_tracking")]
        let mut progress = cell
            .get_resource_mut::<Progress>()
            .expect("Progress resource not found");

        if loaded_handles < loading_asset_handles.handles.len() {
            #[cfg(feature = "progress_tracking")]
            progress.track(loading_asset_handles.handles.len(), loaded_handles);
            return;
        }
        #[cfg(feature = "progress_tracking")]
        progress.persist_done_tasks(loaded_handles);

        let mut state = cell
            .get_resource_mut::<State<T>>()
            .expect("Cannot get State resource");
        let mut asset_loader_configuration = cell
            .get_resource_mut::<AssetLoaderConfiguration<T>>()
            .expect("Cannot get AssetLoaderConfiguration resource");
        if let Some(mut config) = asset_loader_configuration
            .configuration
            .get_mut(state.current())
        {
            config.count -= 1;
            if config.count == 0 {
                if let Some(next) = config.next.as_ref() {
                    state.set(next.clone()).expect("Failed to set next State");
                }
            }
        }
    }
    let asset_collection = Assets::create(world);
    world.insert_resource(asset_collection);
    world.remove_resource::<LoadingAssetHandles<Assets>>();
}

fn init_resource<Asset: FromWorld + Send + Sync + 'static>(world: &mut World) {
    let asset = Asset::from_world(world);
    world.insert_resource(asset);
}

/// A Bevy plugin to configure automatic asset loading
///
/// ```edition2021
/// # use bevy_asset_loader::{AssetLoader, AssetCollection};
/// # use bevy::prelude::*;
/// # use bevy::asset::AssetPlugin;
/// fn main() {
///     let mut app = App::new();
///     AssetLoader::new(GameState::Loading)
///         .continue_to_state(GameState::Menu)
///         .with_collection::<AudioAssets>()
///         .with_collection::<ImageAssets>()
///         .build(&mut app);
///
///     app.add_state(GameState::Loading)
///         .add_plugins(MinimalPlugins)
///         .add_plugin(AssetPlugin::default())
///         .add_system_set(SystemSet::on_enter(GameState::Menu)
///             .with_system(play_audio.system())
///         )
/// #       .set_runner(|mut app| app.schedule.run(&mut app.world))
///         .run();
/// }
///
/// fn play_audio(audio_assets: Res<AudioAssets>, audio: Res<Audio>) {
///     audio.play(audio_assets.background.clone());
/// }
///
/// #[derive(Clone, Eq, PartialEq, Debug, Hash)]
/// enum GameState {
///     Loading,
///     Menu
/// }
///
/// #[derive(AssetCollection)]
/// pub struct AudioAssets {
///     #[asset(path = "audio/background.ogg")]
///     pub background: Handle<AudioSource>,
/// }
///
/// #[derive(AssetCollection)]
/// pub struct ImageAssets {
///     #[asset(path = "images/player.png")]
///     pub player: Handle<Image>,
///     #[asset(path = "images/tree.png")]
///     pub tree: Handle<Image>,
/// }
/// ```
pub struct AssetLoader<T> {
    next_state: Option<T>,
    loading_state: T,
    keys: HashMap<String, String>,
    load: SystemSet,
    check: SystemSet,
    post_process: SystemSet,
    collection_count: usize,
}

impl<State> AssetLoader<State>
where
    State: StateData,
{
    /// Create a new [AssetLoader]
    ///
    /// This function takes a [bevy_ecs::schedule::State] during which all asset collections will
    /// be loaded and inserted as resources.
    /// ```edition2021
    /// # use bevy_asset_loader::{AssetLoader, AssetCollection};
    /// # use bevy::prelude::*;
    /// # use bevy::asset::AssetPlugin;
    /// # fn main() {
    ///     let mut app = App::new();
    ///     AssetLoader::new(GameState::Loading)
    ///         .continue_to_state(GameState::Menu)
    ///         .with_collection::<AudioAssets>()
    ///         .with_collection::<ImageAssets>()
    ///         .build(&mut app);
    /// #   app
    /// #       .add_state(GameState::Loading)
    /// #       .add_plugins(MinimalPlugins)
    /// #       .add_plugin(AssetPlugin::default())
    /// #       .set_runner(|mut app| app.schedule.run(&mut app.world))
    /// #       .run();
    /// # }
    /// # #[derive(Clone, Eq, PartialEq, Debug, Hash)]
    /// # enum GameState {
    /// #     Loading,
    /// #     Menu
    /// # }
    /// # #[derive(AssetCollection)]
    /// # pub struct AudioAssets {
    /// #     #[asset(path = "audio/background.ogg")]
    /// #     pub background: Handle<AudioSource>,
    /// # }
    /// # #[derive(AssetCollection)]
    /// # pub struct ImageAssets {
    /// #     #[asset(path = "images/player.png")]
    /// #     pub player: Handle<Image>,
    /// #     #[asset(path = "images/tree.png")]
    /// #     pub tree: Handle<Image>,
    /// # }
    /// ```
    pub fn new(load: State) -> AssetLoader<State> {
        Self {
            next_state: None,
            loading_state: load.clone(),
            keys: HashMap::default(),
            load: SystemSet::on_enter(load.clone()),
            check: SystemSet::on_update(load.clone()),
            post_process: SystemSet::on_exit(load),
            collection_count: 0,
        }
    }

    /// The [AssetLoader] will set this [bevy_ecs::schedule::State] after all asset collections
    /// are loaded and inserted as resources.
    /// ```edition2021
    /// # use bevy_asset_loader::{AssetLoader, AssetCollection};
    /// # use bevy::prelude::*;
    /// # use bevy::asset::AssetPlugin;
    /// # fn main() {
    ///     let mut app = App::new();
    ///     AssetLoader::new(GameState::Loading)
    ///         .continue_to_state(GameState::Menu)
    ///         .with_collection::<AudioAssets>()
    ///         .with_collection::<ImageAssets>()
    ///         .build(&mut app);
    /// #   app
    /// #       .add_state(GameState::Loading)
    /// #       .add_plugins(MinimalPlugins)
    /// #       .add_plugin(AssetPlugin::default())
    /// #       .set_runner(|mut app| app.schedule.run(&mut app.world))
    /// #       .run();
    /// # }
    /// # #[derive(Clone, Eq, PartialEq, Debug, Hash)]
    /// # enum GameState {
    /// #     Loading,
    /// #     Menu
    /// # }
    /// # #[derive(AssetCollection)]
    /// # pub struct AudioAssets {
    /// #     #[asset(path = "audio/background.ogg")]
    /// #     pub background: Handle<AudioSource>,
    /// # }
    /// # #[derive(AssetCollection)]
    /// # pub struct ImageAssets {
    /// #     #[asset(path = "images/player.png")]
    /// #     pub player: Handle<Image>,
    /// #     #[asset(path = "images/tree.png")]
    /// #     pub tree: Handle<Image>,
    /// # }
    /// ```
    pub fn continue_to_state(mut self, next: State) -> Self {
        self.next_state = Some(next);

        self
    }

    /// Add an [AssetCollection] to the [AssetLoader]
    ///
    /// The added collection will be loaded and inserted into your Bevy app as a resource.
    /// ```edition2021
    /// # use bevy_asset_loader::{AssetLoader, AssetCollection};
    /// # use bevy::prelude::*;
    /// # use bevy::asset::AssetPlugin;
    /// # fn main() {
    ///     let mut app = App::new();
    ///     AssetLoader::new(GameState::Loading)
    ///         .continue_to_state(GameState::Menu)
    ///         .with_collection::<AudioAssets>()
    ///         .with_collection::<ImageAssets>()
    ///         .build(&mut app);
    /// #   app
    /// #       .add_state(GameState::Loading)
    /// #       .add_plugins(MinimalPlugins)
    /// #       .add_plugin(AssetPlugin::default())
    /// #       .set_runner(|mut app| app.schedule.run(&mut app.world))
    /// #       .run();
    /// # }
    /// # #[derive(Clone, Eq, PartialEq, Debug, Hash)]
    /// # enum GameState {
    /// #     Loading,
    /// #     Menu
    /// # }
    /// # #[derive(AssetCollection)]
    /// # pub struct AudioAssets {
    /// #     #[asset(path = "audio/background.ogg")]
    /// #     pub background: Handle<AudioSource>,
    /// # }
    /// # #[derive(AssetCollection)]
    /// # pub struct ImageAssets {
    /// #     #[asset(path = "images/player.png")]
    /// #     pub player: Handle<Image>,
    /// #     #[asset(path = "images/tree.png")]
    /// #     pub tree: Handle<Image>,
    /// # }
    /// ```
    pub fn with_collection<A: AssetCollection>(mut self) -> Self {
        self.load = self
            .load
            .with_system(start_loading::<State, A>.exclusive_system());
        self.check = self
            .check
            .with_system(check_loading_state::<State, A>.exclusive_system());
        self.collection_count += 1;

        self
    }

    /// Insert a map of asset keys with corresponding asset paths
    pub fn add_keys(mut self, mut keys: HashMap<String, String>) -> Self {
        keys.drain().for_each(|(key, value)| {
            self.keys.insert(key, value);
        });

        self
    }

    /// Add any [bevy_ecs::world::FromWorld] resource to be initialized after all asset collections are loaded.
    /// ```edition2021
    /// # use bevy_asset_loader::{AssetLoader, AssetCollection};
    /// # use bevy::prelude::*;
    /// # use bevy::asset::AssetPlugin;
    /// # fn main() {
    ///     let mut app = App::new();
    ///     AssetLoader::new(GameState::Loading)
    ///         .continue_to_state(GameState::Menu)
    ///         .with_collection::<TextureForAtlas>()
    ///         .init_resource::<TextureAtlasFromWorld>()
    ///         .build(&mut app);
    /// #   app
    /// #       .add_state(GameState::Loading)
    /// #       .add_plugins(MinimalPlugins)
    /// #       .add_plugin(AssetPlugin::default())
    /// #       .set_runner(|mut app| app.schedule.run(&mut app.world))
    /// #       .run();
    /// # }
    /// # #[derive(Clone, Eq, PartialEq, Debug, Hash)]
    /// # enum GameState {
    /// #     Loading,
    /// #     Menu
    /// # }
    /// # struct TextureAtlasFromWorld {
    /// #     atlas: Handle<TextureAtlas>
    /// # }
    /// # impl FromWorld for TextureAtlasFromWorld {
    /// #     fn from_world(world: &mut World) -> Self {
    /// #         let cell = world.cell();
    /// #         let assets = cell.get_resource::<TextureForAtlas>().expect("TextureForAtlas not loaded");
    /// #         let mut atlases = cell.get_resource_mut::<Assets<TextureAtlas>>().expect("TextureAtlases missing");
    /// #         TextureAtlasFromWorld {
    /// #             atlas: atlases.add(TextureAtlas::from_grid(assets.array.clone(), Vec2::new(250., 250.), 1, 4))
    /// #         }
    /// #     }
    /// # }
    /// # #[derive(AssetCollection)]
    /// # pub struct TextureForAtlas {
    /// #     #[asset(path = "images/female_adventurer.ogg")]
    /// #     pub array: Handle<Image>,
    /// # }
    /// ```
    pub fn init_resource<A: FromWorld + Send + Sync + 'static>(mut self) -> Self {
        self.post_process = self
            .post_process
            .with_system(init_resource::<A>.exclusive_system());

        self
    }

    /// Finish configuring the [AssetLoader]
    ///
    /// Calling this function is required to set up the asset loading.
    /// ```edition2021
    /// # use bevy_asset_loader::{AssetLoader, AssetCollection};
    /// # use bevy::prelude::*;
    /// # use bevy::asset::AssetPlugin;
    /// # fn main() {
    ///     let mut app = App::new();
    ///     AssetLoader::new(GameState::Loading)
    ///         .continue_to_state(GameState::Menu)
    ///         .with_collection::<AudioAssets>()
    ///         .with_collection::<ImageAssets>()
    ///         .build(&mut app);
    /// #   app
    /// #       .add_state(GameState::Loading)
    /// #       .add_plugins(MinimalPlugins)
    /// #       .add_plugin(AssetPlugin::default())
    /// #       .set_runner(|mut app| app.schedule.run(&mut app.world))
    /// #       .run();
    /// # }
    /// # #[derive(Clone, Eq, PartialEq, Debug, Hash)]
    /// # enum GameState {
    /// #     Loading,
    /// #     Menu
    /// # }
    /// # #[derive(AssetCollection)]
    /// # pub struct AudioAssets {
    /// #     #[asset(path = "audio/background.ogg")]
    /// #     pub background: Handle<AudioSource>,
    /// # }
    /// # #[derive(AssetCollection)]
    /// # pub struct ImageAssets {
    /// #     #[asset(path = "images/player.png")]
    /// #     pub player: Handle<Image>,
    /// #     #[asset(path = "images/tree.png")]
    /// #     pub tree: Handle<Image>,
    /// # }
    /// ```
    pub fn build(self, app: &mut App) {
        let asset_loader_configuration = app
            .world
            .get_resource_mut::<AssetLoaderConfiguration<State>>();
        let config = LoadingConfiguration {
            next: self.next_state.clone(),
            count: 0,
        };
        if let Some(mut asset_loader_configuration) = asset_loader_configuration {
            asset_loader_configuration
                .configuration
                .insert(self.loading_state.clone(), config);
        } else {
            let mut asset_loader_configuration = AssetLoaderConfiguration::default();
            asset_loader_configuration
                .configuration
                .insert(self.loading_state.clone(), config);
            app.world.insert_resource(asset_loader_configuration);
        }
        app.init_resource::<AssetKeys>();
        #[cfg(feature = "progress_tracking")]
        app.init_resource::<Progress>();
        app.add_system_set(self.load.label(AssetLoading::StartLoading))
            .add_system_set(self.check.label(AssetLoading::CheckLoadingState))
            .add_system_set(self.post_process.label(AssetLoading::PostProcess));
    }
}

/// System labels to control the execution order of systems in this Plugin
///
/// Use this to e.g. run your systems before or after `bevy_asset_loader` checks all
/// asset handles for their loading state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, SystemLabel)]
pub enum AssetLoading {
    /// Label given to the system initializing the asset loading `on_enter` of the loading state
    StartLoading,
    /// Label given to the systems checking the loading state of all asset handles `on_update` of the loading state
    CheckLoadingState,
    /// Label given to the systems building the [AssetCollection]s `on_exit` of the loading state
    PostProcess,
}

#[cfg(feature = "render")]
#[doc = include_str!("../../README.md")]
#[cfg(doctest)]
struct ReadmeDoctests;
