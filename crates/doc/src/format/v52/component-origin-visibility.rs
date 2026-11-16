//! Component Origin folders explicitly shown save their complete occurrence paths
//! (`Visibility::shown_component_origins`; they are hidden by default). Older designs have none:
//! nothing new, they round-trip as they are. The last release warns about the newer format
//! number, opens what it can read and keeps the rest.
