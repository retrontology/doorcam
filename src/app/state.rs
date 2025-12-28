use super::{ComponentState, DoorcamOrchestrator};
use std::collections::HashMap;
use tracing::debug;

impl DoorcamOrchestrator {
    /// Update component state
    pub async fn set_component_state(&self, component: &str, state: ComponentState) {
        let mut states = self.component_states.lock().await;
        states.insert(component.to_string(), state.clone());
        debug!("Component '{}' state changed to: {:?}", component, state);
    }

    /// Get component state
    pub async fn get_component_state(&self, component: &str) -> Option<ComponentState> {
        let states = self.component_states.lock().await;
        states.get(component).cloned()
    }

    /// Get all component states
    pub async fn get_all_component_states(&self) -> HashMap<String, ComponentState> {
        let states = self.component_states.lock().await;
        states.clone()
    }
}
