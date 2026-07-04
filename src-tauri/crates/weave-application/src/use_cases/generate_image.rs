use std::sync::Arc;

use tokio::sync::mpsc;

use crate::{
    error::AppResult,
    ports::image_gen_port::{ImageGenPort, ImageProgress, ImageRequest},
};

pub struct GenerateImageUseCase {
    image_gen: Arc<dyn ImageGenPort>,
}

impl GenerateImageUseCase {
    pub fn new(image_gen: Arc<dyn ImageGenPort>) -> Self {
        Self { image_gen }
    }

    pub async fn execute(
        &self,
        request: ImageRequest,
        tx: mpsc::Sender<ImageProgress>,
    ) -> AppResult<()> {
        if !self.image_gen.is_available().await {
            let _ = tx
                .send(ImageProgress::Error(
                    "ComfyUI není dostupný. Zkontroluj, že běží jeho lokální server.".into(),
                ))
                .await;
            return Ok(());
        }
        self.image_gen.generate(request, tx).await
    }
}
