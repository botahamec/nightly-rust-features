use std::{
	collections::HashSet,
	future::Future,
	ops::{Deref, DerefMut},
	pin::Pin,
	sync::Arc,
	task::{Context, Poll, Waker},
};

use dashmap::DashMap;
use parking_lot::Mutex;
use tokio::{
	fs::{read_dir, read_to_string},
	spawn,
};

use crate::models::Feature;

enum LoadProgress {
	Done,
	Loading(HashSet<String>),
}

impl LoadProgress {
	fn is_done(&self) -> bool {
		matches!(self, Self::Done)
	}
}

pub struct FeatureManager {
	cache: DashMap<String, Feature>,
	load_progress: Mutex<LoadProgress>,
	done_wakers: Mutex<Vec<Waker>>,
	feature_wakers: DashMap<String, Vec<Waker>>,
}

pub struct FeatureListFuture {
	feature_manager: Arc<FeatureManager>,
}

impl Future for FeatureListFuture {
	type Output = Box<[Feature]>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		match self.feature_manager.load_progress.lock().deref() {
			LoadProgress::Done => Poll::Ready(
				self.feature_manager
					.cache
					.iter()
					.map(|kv| kv.value().clone())
					.collect(),
			),
			LoadProgress::Loading(_) => {
				self.feature_manager
					.done_wakers
					.lock()
					.push(cx.waker().clone());
				Poll::Pending
			}
		}
	}
}

pub struct FeatureFuture {
	feature_manager: Arc<FeatureManager>,
	feature_name: String,
}

impl Future for FeatureFuture {
	type Output = Option<Feature>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		if self.feature_manager.is_done_loading() {
			Poll::Ready(
				self.feature_manager
					.cache
					.get(&self.feature_name)
					.as_deref()
					.cloned(),
			)
		} else {
			match self
				.feature_manager
				.feature_wakers
				.get_mut(&self.feature_name)
			{
				Some(mut wakers) => wakers.push(cx.waker().clone()),
				None => {
					self.feature_manager
						.feature_wakers
						.insert(self.feature_name.clone(), vec![cx.waker().clone()]);
				}
			};

			Poll::Pending
		}
	}
}

impl FeatureManager {
	pub fn new() -> Arc<Self> {
		let this = Self {
			cache: DashMap::new(),
			load_progress: Mutex::new(LoadProgress::Loading(HashSet::new())),
			done_wakers: Mutex::new(Vec::new()),
			feature_wakers: DashMap::new(),
		};

		let this = Arc::new(this);
		let clone = this.clone();

		spawn(async move {
			clone.load_all_features().await;
		});

		this
	}

	async fn load_all_features(&self) {
		let mut files = read_dir("static/features").await.unwrap();
		while let Some(entry) = files.next_entry().await.unwrap() {
			if let Ok(metadata) = entry.metadata().await {
				if metadata.is_file() {
					self.load_feature(entry.file_name().to_string_lossy().deref())
						.await;
				} else {
					eprintln!(
						"Failed to load metadata for {}",
						entry.file_name().to_string_lossy()
					)
				}
			}
		}

		dbg!(&self.cache);

		let mut load_progress = self.load_progress.lock();
		*load_progress = LoadProgress::Done;

		for waker in self.done_wakers.lock().iter() {
			waker.clone().wake();
		}

		for (_, wakers) in self.feature_wakers.clone() {
			for waker in wakers {
				waker.wake();
			}
		}
	}

	async fn load_feature(&self, feature_name: &str) {
		// check to see if the feature is already being loaded
		{
			let mut load_progress = self.load_progress.lock();
			match load_progress.deref_mut() {
				LoadProgress::Done => return,
				LoadProgress::Loading(ref mut features) => {
					if features.contains(feature_name) {
						return;
					} else {
						features.insert(feature_name.to_string());
					}
				}
			}
		}

		// load the feature
		let json = read_to_string(format!("static/features/{}", feature_name))
			.await
			.unwrap();
		let feature = serde_json::from_str::<Feature>(&json);

		// log any failed parsing
		if let Err(e) = feature {
			eprintln!("{}", e);
			return;
		}

		// cache the result
		let feature = feature.unwrap();
		self.cache.insert(feature_name.to_string(), feature);

		// update the loading progress
		{
			let mut load_progress = self.load_progress.lock();
			if let LoadProgress::Loading(ref mut features) = load_progress.deref_mut() {
				features.remove(feature_name);
			}
		}

		// wake the wakers
		if let Some(wakers) = self.feature_wakers.get(feature_name) {
			for waker in wakers.value() {
				waker.clone().wake()
			}
		}
	}

	pub async fn all_features(self: Arc<Self>) -> Box<[Feature]> {
		FeatureListFuture {
			feature_manager: self.clone(),
		}
		.await
	}

	pub fn is_done_loading(&self) -> bool {
		self.load_progress.lock().deref().is_done()
	}

	pub async fn get_feature(self: Arc<Self>, feature_name: &str) -> Option<Feature> {
		FeatureFuture {
			feature_manager: self.clone(),
			feature_name: feature_name.to_string(),
		}
		.await
	}
}
