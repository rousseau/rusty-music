// SPDX-License-Identifier: GPL-3.0-or-later
//! Produit « mince » `x · wᵀ` pour quelques lignes : `x` `[m, k]` (m ≤ 16,
//! les segments d'un lot), `w` `[n, k]` (disposition PyTorch).
//!
//! Burn a un noyau gemv rapide pour m = 1, mais dès m = 2 il passe au
//! produit tuilé général, deux à huit fois plus lent sur Metal pour nos
//! formes (`examples/micro_matmul.rs`) alors que le coût devrait rester
//! celui de la lecture des poids. Ce noyau lit chaque ligne de `w` une fois
//! pour les m lignes de `x` : un plan (32 fils) pour quatre colonnes de
//! sortie, accumulation en f32, réduction par `plane_sum`. Mesuré en f16 sur
//! M4 Pro, m = 16 : 0,04 à 0,08 ms par produit contre 0,11 à 0,38 ms.
//!
//! Hors de son domaine (m = 1, préremplissage, matériel sans opérations de
//! plan de taille fixe), c'est le produit de Burn.

use burn::tensor::backend::Backend;
use burn::tensor::ops::FloatTensor;

/// Le backend sait faire le produit mince ; par défaut, le produit de Burn.
pub trait ProduitMince: Backend {
    /// `x` `[m, k]` · `w` `[n, k]`ᵀ → `[m, n]`.
    fn produit_mince(x: FloatTensor<Self>, w: FloatTensor<Self>) -> FloatTensor<Self> {
        Self::float_matmul(x, Self::float_transpose(w))
    }
}

#[cfg(feature = "cpu")]
impl ProduitMince for burn::backend::NdArray {}

#[cfg(feature = "gpu")]
mod cube {
    use burn::tensor::ops::{FloatTensor, FloatTensorOps};
    use burn::tensor::Shape;
    use burn_cubecl::ops::numeric::empty_device_dtype;
    use burn_cubecl::{BoolElement, CubeBackend, CubeRuntime, FloatElement, IntElement};
    use cubecl::features::Plane;
    use cubecl::prelude::*;

    use super::ProduitMince;

    /// Lignes de `x` au plus pour le noyau ; au-delà, le produit de Burn.
    const M_MAX: usize = 16;
    /// Colonnes de sortie par plan.
    const COLONNES: usize = 4;

    /// Le noyau sert-il pour `m` lignes ?
    fn domaine(m: usize) -> bool {
        (2..=M_MAX).contains(&m) && std::env::var_os("MUSCRIPTOR_SANS_MINCE").is_none()
    }

    /// Un plan par groupe de `c` colonnes de sortie : chaque fil parcourt
    /// `k` par pas de `PLANE_DIM` vecteurs, lit chaque vecteur de `x` une
    /// fois pour les `c` colonnes, et garde `m · c` sommes en f32.
    #[cube(launch_unchecked)]
    fn noyau<F: Float, N: Size>(
        x: &Tensor<Vector<F, N>>,
        w: &Tensor<Vector<F, N>>,
        sortie: &mut Tensor<F>,
        #[comptime] m: usize,
        #[comptime] c: usize,
        #[define(F)] _dtype: StorageType,
    ) {
        let n = sortie.shape(1);
        let premiere = (CUBE_POS_X as usize * CUBE_DIM_Y as usize + UNIT_POS_Y as usize) * c;
        if premiere >= n {
            terminate!();
        }
        let k = w.shape(1) / w.vector_size();
        let mut acc = Array::<f32>::new(m * c);
        #[unroll]
        for a in 0..m * c {
            acc[a] = 0.0;
        }
        let mut j = UNIT_POS_X as usize;
        while j < k {
            #[unroll]
            for i in 0..m {
                let v = Vector::<f32, N>::cast_from(x[i * k + j]);
                #[unroll]
                for q in 0..c {
                    // Les colonnes au-delà de `n` relisent la dernière.
                    let col = select(premiere + q < n, premiere + q, n - 1);
                    acc[i * c + q] += f32::cast_from(
                        (v * Vector::<f32, N>::cast_from(w[col * k + j])).vector_sum(),
                    );
                }
            }
            j += PLANE_DIM as usize;
        }
        #[unroll]
        for a in 0..m * c {
            let s = plane_sum(acc[a]);
            let (i, col) = (a / c, premiere + a % c);
            if UNIT_POS_X == 0 && col < n {
                sortie[i * n + col] = F::cast_from(s);
            }
        }
    }

    impl<R: CubeRuntime, F: FloatElement, I: IntElement, BT: BoolElement> ProduitMince
        for CubeBackend<R, F, I, BT>
    {
        fn produit_mince(x: FloatTensor<Self>, w: FloatTensor<Self>) -> FloatTensor<Self> {
            let (m, k) = (x.meta.shape()[0], x.meta.shape()[1]);
            let n = w.meta.shape()[0];
            let props = x.client.properties();
            let (plan, plan_min) = (props.hardware.plane_size_max, props.hardware.plane_size_min);
            // Vecteurs de 4 : la largeur maximale de WGSL.
            let largeur = 4;
            let adapte = domaine(m)
                && plan == plan_min
                && props.features.plane.contains(Plane::Ops)
                && x.dtype == w.dtype
                && x.is_contiguous()
                && w.is_contiguous()
                && k % largeur == 0;
            if !adapte {
                return Self::float_matmul(x, Self::float_transpose(w));
            }
            let sortie = empty_device_dtype(
                x.client.clone(),
                x.device.clone(),
                Shape::new([m, n]),
                x.dtype,
            );
            let (plans, c) = (8, COLONNES);
            let cube_dim = CubeDim::new_2d(plan, plans);
            let cube_count = CubeCount::Static(n.div_ceil(plans as usize * c) as u32, 1, 1);
            let dtype = x.dtype;
            unsafe {
                noyau::launch_unchecked(
                    &sortie.client,
                    cube_count,
                    cube_dim,
                    largeur,
                    x.into_tensor_arg(),
                    w.into_tensor_arg(),
                    sortie.clone().into_tensor_arg(),
                    m,
                    c,
                    dtype.into(),
                );
            }
            sortie
        }
    }

    mod fusion {
        use burn::tensor::ops::{FloatTensor, FloatTensorOps};
        use burn::tensor::Shape;
        use burn_fusion::stream::{Operation, OperationStreams};
        use burn_fusion::{Fusion, FusionBackend, FusionRuntime};
        use burn_ir::{CustomOpIr, HandleContainer, OperationIr, OperationOutput, TensorIr};

        use super::super::ProduitMince;
        use super::domaine;

        /// Opération personnalisée dans le flux de la fusion ; hors du
        /// domaine du noyau, le produit de Burn reste fusionnable.
        impl<B: FusionBackend + ProduitMince> ProduitMince for Fusion<B> {
            fn produit_mince(x: FloatTensor<Self>, w: FloatTensor<Self>) -> FloatTensor<Self> {
                if !domaine(x.shape[0]) {
                    return Self::float_matmul(x, Self::float_transpose(w));
                }
                #[derive(Clone, Debug)]
                struct Op<B> {
                    desc: CustomOpIr,
                    _b: core::marker::PhantomData<B>,
                }

                impl<B1: FusionBackend + ProduitMince> Operation<B1::FusionRuntime> for Op<B1> {
                    fn execute(
                        &self,
                        handles: &mut HandleContainer<
                            <B1::FusionRuntime as FusionRuntime>::FusionHandle,
                        >,
                    ) {
                        let ([x, w], [sortie]) = self.desc.as_fixed();
                        let y = B1::produit_mince(
                            handles.get_float_tensor::<B1>(x),
                            handles.get_float_tensor::<B1>(w),
                        );
                        handles.register_float_tensor::<B1>(&sortie.id, y);
                    }
                }

                let client = x.client.clone();
                let (m, n, dtype) = (x.shape[0], w.shape[0], x.dtype);
                let streams = OperationStreams::with_inputs([&x, &w]);
                let sortie =
                    TensorIr::uninit(client.create_empty_handle(), Shape::new([m, n]), dtype);
                let desc = CustomOpIr::new("produit_mince", &[x.into_ir(), w.into_ir()], &[sortie]);
                client
                    .register(
                        streams,
                        OperationIr::Custom(desc.clone()),
                        Op::<B> {
                            desc,
                            _b: core::marker::PhantomData,
                        },
                    )
                    .output()
            }
        }
    }
}
