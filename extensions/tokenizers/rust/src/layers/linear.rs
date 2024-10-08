use crate::layers::cublaslt::get_cublas_lt_wrapper;
use candle::{Device, Result, Tensor};
use candle_nn::VarBuilder;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum HiddenAct {
    Gelu,
    Relu,
    #[serde(alias = "silu")]
    Swiglu,
}

#[derive(Debug)]
pub struct Linear {
    weight: Tensor,
    bias: Option<Tensor>,
    act: Option<HiddenAct>,
    span: tracing::Span,
}

impl Linear {
    pub fn load(
        vb: VarBuilder,
        in_dim: usize,
        out_dim: usize,
        act: Option<HiddenAct>,
    ) -> Result<Self> {
        let bias = match vb.get(out_dim, "bias") {
            Ok(w) => Some(w),
            Err(_) => None,
        };
        Ok(Self {
            weight: vb.get((out_dim, in_dim), "weight")?,
            bias,
            act,
            span: tracing::span!(tracing::Level::TRACE, "linear"),
        })
    }

    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let _enter = self.span.enter();

        #[allow(unused)]
        if let (Device::Cuda(_), Some(cublaslt)) = (x.device(), get_cublas_lt_wrapper()) {
            match x.dims() {
                &[bsize, _, _] => cublaslt.batch_matmul(
                    &self.weight.broadcast_left(bsize)?,
                    x,
                    None,
                    None,
                    None,
                    self.bias.as_ref(),
                    self.act.clone(),
                ),
                _ => cublaslt.matmul(
                    &self.weight,
                    x,
                    None,
                    None,
                    None,
                    self.bias.as_ref(),
                    self.act.clone(),
                ),
            }
        } else {
            let w = match x.dims() {
                &[bsize, _, _] => self.weight.broadcast_left(bsize)?.t()?,
                _ => self.weight.t()?,
            };
            let x = x.matmul(&w)?;
            let x = match &self.bias {
                None => Ok(x),
                Some(bias) => x.broadcast_add(bias),
            }?;
            if let Some(act) = &self.act {
                match act {
                    HiddenAct::Gelu => x.gelu(),
                    HiddenAct::Relu => x.relu(),
                    HiddenAct::Swiglu => candle_nn::ops::swiglu(&x),
                }
            } else {
                Ok(x)
            }
        }
    }
}
