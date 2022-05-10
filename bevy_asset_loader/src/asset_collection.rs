use crate::asset_loader::DynamicAssets;
use bevy::app::App;
use bevy::asset::HandleUntyped;
use bevy::prelude::World;

/// Trait to mark a struct as a collection of assets
///
/// Derive is supported for structs with named fields.
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
    /// Create a new asset collection from the [`AssetServer`](::bevy::asset::AssetServer)
    fn create(world: &mut World) -> Self;
    /// Start loading all the assets in the collection
    fn load(world: &mut World) -> Vec<HandleUntyped>;
}

/// Extension trait for [`App`](bevy::app::App) enabling initialisation of [asset collections](AssetCollection)
pub trait AssetCollectionApp {
    /// Initialise an [`AssetCollection`]
    ///
    /// This function does not give any guaranties about the loading status of the asset handles.
    /// If you want to use a loading state, you do not need this function! Instead use an [`AssetLoader`](crate::AssetLoader)
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
            self.init_resource::<DynamicAssets>();
            // make sure the assets start to load
            let _ = Collection::load(&mut self.world);
            let resource = Collection::create(&mut self.world);
            self.insert_resource(resource);
        }
        self
    }
}

/// Extension trait for [`World`](bevy::ecs::world::World) enabling initialisation of [asset collections](AssetCollection)
pub trait AssetCollectionWorld {
    /// Initialise an [`AssetCollection`]
    ///
    /// This function does not give any guaranties about the loading status of the asset handles.
    /// If you want to use a loading state, you do not need this function! Instead use an [`AssetLoader`](crate::AssetLoader)
    /// and add collections to it to be prepared during the loading state.
    fn init_collection<A: AssetCollection>(&mut self);
}

impl AssetCollectionWorld for World {
    fn init_collection<A: AssetCollection>(&mut self) {
        if self.get_resource::<A>().is_none() {
            if self.get_resource::<DynamicAssets>().is_none() {
                // This resource is required for loading a collection
                // Since bevy_asset_loader does not have a "real" Plugin,
                // we need to make sure the resource exists here
                self.insert_resource(DynamicAssets::default());
            }
            // make sure the assets start to load
            let _ = A::load(self);
            let collection = A::create(self);
            self.insert_resource(collection);
        }
    }
}
