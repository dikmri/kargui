//! GPU の道。Direct3D 11 を直接使い、描く物の一覧を 1 回の命令（インスタンス描画）で描く。
//! wgpu などの中間の層は使わない（前回の比べで、wgpu は起動に約 1 秒、GPU のメモリに約 240 MB かかったため）。
//! シェーダーは組み立て済みの物（shaders/*.cso）を埋め込む。

use std::time::Instant;

use crate::log;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::{Interface, Result, s};

use crate::geom::Color;
use crate::paint::{DrawList, Quad};
use crate::text::Atlas;

const VS: &[u8] = include_bytes!("../shaders/ui_vs.cso");
const PS: &[u8] = include_bytes!("../shaders/ui_ps.cso");

pub(crate) struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    target: Option<ID3D11RenderTargetView>,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    frame: ID3D11Buffer,
    instances: ID3D11Buffer,
    capacity: usize,
    atlas: ID3D11Texture2D,
    atlas_view: ID3D11ShaderResourceView,
    sampler: ID3D11SamplerState,
    blend: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    output: Option<IDXGIOutput>,
    pub width: u32,
    pub height: u32,
}

fn buffer(device: &ID3D11Device, bytes: u32, bind: D3D11_BIND_FLAG) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: bytes,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: bind.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut out = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut out))? };
    Ok(out.unwrap())
}

impl Gpu {
    pub fn new(hwnd: HWND, width: u32, height: u32, atlas_size: u32) -> Result<Self> {
        let t0 = Instant::now();
        let mut device = None;
        let mut context = None;
        let levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_SINGLETHREADED,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?
        };
        let device: ID3D11Device = device.unwrap();
        let context: ID3D11DeviceContext = context.unwrap();
        let t_device = t0.elapsed().as_secs_f64() * 1000.0;

        let dxgi_device: IDXGIDevice1 = device.cast()?;
        unsafe { dxgi_device.SetMaximumFrameLatency(1)? };
        let adapter = unsafe { dxgi_device.GetAdapter()? };
        let adapter_name = unsafe { adapter.GetDesc() }
            .map(|d| String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_string())
            .unwrap_or_default();
        let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width.max(1),
            Height: height.max(1),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_NONE,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: 0,
        };
        let swapchain = unsafe { factory.CreateSwapChainForHwnd(&device, hwnd, &desc, None, None)? };
        unsafe { factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)? };
        let t_swap = t0.elapsed().as_secs_f64() * 1000.0;

        let mut vs = None;
        let mut ps = None;
        let mut input_layout = None;
        unsafe {
            device.CreateVertexShader(VS, None, Some(&mut vs))?;
            device.CreatePixelShader(PS, None, Some(&mut ps))?;
            let element = |name, offset| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: name,
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: offset,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            };
            let elements = [element(s!("RECT"), 0), element(s!("COLOR"), 16), element(s!("UV"), 32), element(s!("EXTRA"), 48)];
            device.CreateInputLayout(&elements, VS, Some(&mut input_layout))?;
        }

        let frame = buffer(&device, 16, D3D11_BIND_CONSTANT_BUFFER)?;
        let capacity = 1024;
        let instances = buffer(&device, (capacity * size_of::<Quad>()) as u32, D3D11_BIND_VERTEX_BUFFER)?;

        let tex_desc = D3D11_TEXTURE2D_DESC {
            Width: atlas_size,
            Height: atlas_size,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut atlas = None;
        let mut atlas_view = None;
        let mut sampler = None;
        let mut blend = None;
        let mut raster = None;
        unsafe {
            device.CreateTexture2D(&tex_desc, None, Some(&mut atlas))?;
            device.CreateShaderResourceView(atlas.as_ref().unwrap(), None, Some(&mut atlas_view))?;
            let sd = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_POINT,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MaxLOD: f32::MAX,
                ..Default::default()
            };
            device.CreateSamplerState(&sd, Some(&mut sampler))?;
            let mut bd = D3D11_BLEND_DESC::default();
            bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_ONE,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            device.CreateBlendState(&bd, Some(&mut blend))?;
            let rd = D3D11_RASTERIZER_DESC {
                FillMode: D3D11_FILL_SOLID,
                CullMode: D3D11_CULL_NONE,
                DepthClipEnable: true.into(),
                ScissorEnable: true.into(),
                ..Default::default()
            };
            device.CreateRasterizerState(&rd, Some(&mut raster))?;
        }
        log::event(
            "gpu.init",
            &format!("adapter={adapter_name} size={width}x{height}"),
            &format!("device_ms={t_device:.1} swapchain_ms={t_swap:.1} total_ms={:.1}", t0.elapsed().as_secs_f64() * 1000.0),
        );
        let mut gpu = Self {
            device,
            context,
            swapchain,
            target: None,
            vs: vs.unwrap(),
            ps: ps.unwrap(),
            input_layout: input_layout.unwrap(),
            frame,
            instances,
            capacity,
            atlas: atlas.unwrap(),
            atlas_view: atlas_view.unwrap(),
            sampler: sampler.unwrap(),
            blend: blend.unwrap(),
            raster: raster.unwrap(),
            output: None,
            width,
            height,
        };
        gpu.make_target()?;
        Ok(gpu)
    }

    fn make_target(&mut self) -> Result<()> {
        let back: ID3D11Texture2D = unsafe { self.swapchain.GetBuffer(0)? };
        let mut target = None;
        unsafe { self.device.CreateRenderTargetView(&back, None, Some(&mut target))? };
        self.target = target;
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 || (width == self.width && height == self.height) {
            return Ok(());
        }
        self.target = None;
        unsafe {
            self.context.OMSetRenderTargets(None, None);
            self.swapchain.ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
        }
        self.width = width;
        self.height = height;
        self.output = None;
        self.make_target()
    }

    fn upload_atlas(&mut self, atlas: &mut Atlas) {
        let Some((top, bottom)) = atlas.dirty.take() else { return };
        let b = D3D11_BOX { left: 0, top, front: 0, right: atlas.size, bottom, back: 1 };
        let src = atlas.pixels[(top * atlas.size) as usize..].as_ptr();
        unsafe { self.context.UpdateSubresource(&self.atlas, 0, Some(&b), src.cast(), atlas.size, 0) };
    }

    /// 一覧を全部描き、画面に出す（Present）。sync が 0 なら待たずに出す
    pub fn render(&mut self, list: &DrawList, atlas: &mut Atlas, background: Color, sync: u32) -> Result<()> {
        self.upload_atlas(atlas);
        // 画像はまだ GPU の道では描かない（シェーダーが画像を読めない）。番号の並びは崩さずに透明にする
        let hidden: Vec<Quad>;
        let quads: &[Quad] = if list.images.is_empty() {
            &list.quads
        } else {
            static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                crate::log::event("gpu.image", &format!("images={}", list.images.len()), "not drawn on the Direct3D 11 renderer yet");
            }
            hidden = list.quads.iter().map(|q| if q.image_index().is_some() { Quad { color: [0.0; 4], uv: [0.0; 4], extra: [0.0; 4], ..*q } } else { *q }).collect();
            &hidden
        };
        let n = quads.len();
        if n > self.capacity {
            self.capacity = n.next_power_of_two();
            self.instances = buffer(&self.device, (self.capacity * size_of::<Quad>()) as u32, D3D11_BIND_VERTEX_BUFFER)?;
        }
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&self.instances, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(quads.as_ptr(), mapped.pData.cast::<Quad>(), n);
            self.context.Unmap(&self.instances, 0);

            let frame = [self.width as f32, self.height as f32, atlas.size as f32, atlas.size as f32];
            self.context.Map(&self.frame, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(frame.as_ptr(), mapped.pData.cast::<f32>(), 4);
            self.context.Unmap(&self.frame, 0);

            let target = self.target.clone();
            let c = &self.context;
            c.OMSetRenderTargets(Some(&[target.clone()]), None);
            let bg = [background[0], background[1], background[2], 1.0];
            c.ClearRenderTargetView(target.as_ref().unwrap(), &bg);
            let viewport = D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            c.RSSetViewports(Some(&[viewport]));
            c.RSSetState(&self.raster);
            c.IASetInputLayout(&self.input_layout);
            c.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            let stride = size_of::<Quad>() as u32;
            let offset = 0u32;
            let buffers = [Some(self.instances.clone())];
            c.IASetVertexBuffers(0, 1, Some(buffers.as_ptr()), Some(&stride), Some(&offset));
            c.VSSetShader(&self.vs, None);
            c.VSSetConstantBuffers(0, Some(&[Some(self.frame.clone())]));
            c.PSSetShader(&self.ps, None);
            c.PSSetShaderResources(0, Some(&[Some(self.atlas_view.clone())]));
            c.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            c.OMSetBlendState(&self.blend, None, 0xffff_ffff);
            // 層ごとに切り取り範囲（シザー矩形）を決めて描く
            for layer in &list.layers {
                if layer.end <= layer.start || layer.clip.is_empty() {
                    continue;
                }
                let r = layer.clip;
                let scissor = windows::Win32::Foundation::RECT { left: r.x, top: r.y, right: r.right(), bottom: r.bottom() };
                c.RSSetScissorRects(Some(&[scissor]));
                c.DrawInstanced(4, (layer.end - layer.start) as u32, 0, layer.start as u32);
            }
            let hr = self.swapchain.Present(sync, DXGI_PRESENT(0));
            if hr.is_err() {
                log::event("gpu.present", &format!("sync={sync}"), &format!("error={hr:?}"));
                return hr.ok();
            }
        }
        Ok(())
    }

    /// 窓のあるモニターの垂直同期を待つ。窓が別のモニターへ動いたら output を取り直す
    pub fn wait_vblank(&mut self) {
        if self.output.is_none() {
            self.output = unsafe { self.swapchain.GetContainingOutput() }.ok();
        }
        if let Some(o) = &self.output {
            if unsafe { o.WaitForVBlank() }.is_err() {
                self.output = None;
            }
        }
    }

    /// 窓が動いたあとに呼ぶ
    pub fn forget_output(&mut self) {
        self.output = None;
    }
}
