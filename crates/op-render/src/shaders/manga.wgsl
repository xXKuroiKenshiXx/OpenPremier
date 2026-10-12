// Native image animation and compositing tools, in normalized layer coordinates.
@group(0) @binding(2) var tex0: texture_2d<f32>;
@group(0) @binding(3) var tex1: texture_2d<f32>;
fn manga_sample(uv: vec2<f32>, clear: bool) -> vec4<f32> {
    if (clear && (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)))) {return vec4<f32>(0.0);}
    return textureSampleLevel(tex0,samp,clamp(uv,vec2<f32>(0.0),vec2<f32>(1.0)),0.0);
}
@fragment
fn fs_manga_camera(in: VOut) -> @location(0) vec4<f32> {
    var t=clamp(u.time/max(prm(4),0.01),0.0,1.0);
    if (prm(5)>0.5){t=t*t*(3.0-2.0*t);}
    let dir=i32(prm(0));
    let sign=select(1.0,-1.0,dir==1 || dir==3);
    let vertical=dir>=2;
    let travel=(t-0.5)*prm(1)*sign;
    let tilt=sin(prm(3)*PI/180.0)*(2.0*t-1.0);
    var p=(in.uv-0.5)/max(prm(2),0.1);
    let perspective=max(0.25,1.0+select(p.x,p.y,vertical)*tilt);
    p=p/perspective;
    p=p+select(vec2<f32>(travel,0.0),vec2<f32>(0.0,travel),vertical);
    return manga_sample(p+0.5,prm(6)>0.5);
}
@fragment
fn fs_parallax(in: VOut) -> @location(0) vec4<f32> {
    let cam=prm2(0)-0.5;
    let base=(in.uv-0.5)/max(prm(4),0.1);
    let perspective=max(0.25,1.0+base.x*sin(prm(6)*PI/180.0)+base.y*sin(prm(5)*PI/180.0));
    var uv=base/perspective+0.5+cam*prm(2);
    // Luma is an optional depth approximation. Separate alpha layers give real occlusion.
    for (var i=0;i<3;i=i+1){let c=unpremul(manga_sample(uv,false));uv=base/perspective+0.5+cam*(prm(2)+(luma(c.rgb)-0.5)*prm(3));}
    return manga_sample(uv,prm(7)>0.5);
}
@fragment
fn fs_manga_frame(in: VOut) -> @location(0) vec4<f32> {
    let c=manga_sample(in.uv,false);
    let d=min(in.uv,1.0-in.uv);
    let f=max(prm(2),0.00001);
    let border=1.0-smoothstep(prm(0),prm(0)+f,d.x)*smoothstep(prm(1),prm(1)+f,d.y);
    let radius=length((in.uv-0.5)*1.41421356);
    let vignette=smoothstep(0.25,1.0,radius)*prm(3);
    let amount=max(border,vignette)*prm(7);
    return vec4<f32>(mix(c.rgb,prm3(4)*c.a,amount),c.a);
}
@fragment
fn fs_dialogue_occlusion(in: VOut) -> @location(0) vec4<f32> {
    let p=abs(in.uv-prm2(0))/max(prm2(2)*0.5,vec2<f32>(0.00001));
    let d=select(length(p),max(p.x,p.y),prm(5)>0.5);
    let feather=max(prm(4),0.00001);
    let keep=smoothstep(1.0-feather,1.0+feather,d);
    return manga_sample(in.uv,false)*keep;
}
