//! Components may save an optional `identification_color` (sRGB bytes), separate from their
//! appearances. Older documents omit it and use a stable colour derived from the component ID;
//! no rewrite is needed. Released readers refuse the newer format instead of losing overrides.
