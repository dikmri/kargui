// kargui の試作: 描く物 1 つ（角の丸い四角か、文字の形）を 4 つの頂点の四角で描く。
// 頂点の入力は 1 つの描く物ごと（インスタンスごと）。CPU の道（raster.rs）と同じ計算にする。
// 組み立て: shaders/compile.ps1（Windows SDK の fxc.exe）。機能レベル 10.0 でも動くように 4_0 で組み立てる。

cbuffer Frame : register(b0)
{
    float2 viewport_size; // 窓の大きさ（画素）
    float2 atlas_size;    // 文字の形の表の大きさ（画素）
};

Texture2D<float> atlas : register(t0);
SamplerState point_sampler : register(s0);

struct VertexOut
{
    float4 position : SV_Position;
    float2 local : LOCAL;          // 四角の左上からの位置（画素）
    float2 uv : TEXCOORD0;         // 文字の形の表の中の位置（0〜1）
    nointerpolation float4 color : COLOR;
    nointerpolation float2 size : SIZE;      // 四角の幅と高さ（画素）
    nointerpolation float2 params : PARAMS;  // x: 角の丸み、y: 1 なら文字の形
};

VertexOut vs_main(uint vertex_id : SV_VertexID,
                  float4 rect : RECT,
                  float4 color : COLOR,
                  float4 uv : UV,
                  float4 extra : EXTRA)
{
    // 三角形の帯の順: (0,0) (1,0) (0,1) (1,1)
    float2 corner = float2(vertex_id & 1, vertex_id >> 1);
    float2 pixel = rect.xy + corner * rect.zw;
    VertexOut o;
    o.position = float4(pixel / viewport_size * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    o.local = corner * rect.zw;
    o.uv = (uv.xy + corner * uv.zw) / atlas_size;
    o.color = color;
    o.size = rect.zw;
    o.params = float2(extra.x, uv.z > 0.0 ? 1.0 : 0.0);
    return o;
}

float4 ps_main(VertexOut i) : SV_Target
{
    float coverage;
    if (i.params.y > 0.5)
    {
        coverage = atlas.Sample(point_sampler, i.uv);
    }
    else
    {
        // 角の丸い四角の符号付き距離。0.5 画素の幅でなめらかにする
        float radius = i.params.x;
        float2 half_size = i.size * 0.5;
        float2 q = abs(i.local - half_size) - (half_size - radius);
        float distance = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
        coverage = saturate(0.5 - distance);
    }
    float alpha = i.color.a * coverage;
    // 乗算済みの色で返す（混ぜ方は ONE と INV_SRC_ALPHA）
    return float4(i.color.rgb * alpha, alpha);
}
