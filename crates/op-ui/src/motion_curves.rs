//! Alternate animation view. Edits native keyframes and participates in project undo.
use crate::{
    app::State,
    effect_controls::ClipCtx,
    i18n::{t, tn},
    theme,
};
use egui::{Color32, Sense, Stroke, pos2, vec2};
use op_application::plugins::{BUILTIN_CURVES, CurvePreset};
use op_core::*;

#[derive(Default)]
pub struct CurvesView {
    pub open: bool,
    parameter: String,
    clip: Option<ClipId>,
    segment: usize,
    axis: usize,
    speed: bool,
    preset_name: String,
    drag_range: Option<(f64, f64)>,
}
impl CurvesView {
    pub fn choose(&mut self, clip: ClipId, comp: ComponentId, key: &str, segment: usize) {
        self.clip = Some(clip);
        self.parameter = format!("{}:{key}", comp.0);
        self.segment = segment;
        self.open = true;
    }
}
fn scalar(v: &Value, axis: usize) -> f64 {
    match v {
        Value::Point(p) => p[axis.min(1)],
        Value::Color(c) => [c.r, c.g, c.b, c.a][axis.min(3)] as f64,
        _ => v.as_f64(),
    }
}
fn distance(a: &Value, b: &Value) -> f64 {
    match (a, b) {
        (Value::Point(a), Value::Point(b)) => (b[0] - a[0]).hypot(b[1] - a[1]),
        (Value::Color(a), Value::Color(b)) => {
            ((b.r - a.r).powi(2) + (b.g - a.g).powi(2) + (b.b - a.b).powi(2) + (b.a - a.a).powi(2))
                .sqrt() as f64
        }
        _ => b.as_f64() - a.as_f64(),
    }
}
fn handles(a: &Keyframe, b: &Keyframe) -> [f64; 4] {
    let span = (b.time - a.time).seconds();
    let delta = distance(&a.value, &b.value);
    if delta.abs() < 1e-12 {
        return BUILTIN_CURVES[0].1;
    }
    let out = if matches!(a.interp_out, Interp::Bezier | Interp::ContinuousBezier) {
        [
            a.ease_out.influence,
            a.ease_out.speed * span * a.ease_out.influence / delta,
        ]
    } else {
        [1.0 / 3.0, 1.0 / 3.0]
    };
    let inc = if matches!(b.interp_in, Interp::Bezier | Interp::ContinuousBezier) {
        [
            1.0 - b.ease_in.influence,
            1.0 - b.ease_in.speed * span * b.ease_in.influence / delta,
        ]
    } else {
        [2.0 / 3.0, 2.0 / 3.0]
    };
    [out[0], out[1], inc[0], inc[1]]
}
pub fn show(s: &mut State, ctx: &egui::Context) {
    if !s.ed.prefs.plugins.motion_curves {
        s.curves.open = false;
    }
    if !s.curves.open {
        return;
    }
    let mut open = true;
    egui::Window::new(t("Motion Curves")).id(egui::Id::new("motion-curves")).open(&mut open).default_width(660.0).show(ctx,|ui| {
        let Some(sid)=s.ed.active else{ui.label(t("No sequence open"));return;};
        let Some(clip)=s.curves.clip.filter(|id|s.ed.selection.clips.contains(id)).or_else(||s.ed.selection.clips.first().copied()) else{ui.label(t("Select a clip to see its effects"));return;};
        let Some(cx)=ClipCtx::new(s,sid,clip) else{return;};
        let options:Vec<_>=cx.clip.components.iter().flat_map(|c| c.params.iter().filter(|p|p.animated&&p.keys.len()>=2&&p.value.interpolates())
            .map(move |p| (format!("{}:{}",c.id.0,p.key),c,p,format!("{} / {}",c.def().map(|d|tn(d.name)).unwrap_or_else(||c.effect.clone()),c.def().and_then(|d|d.param(&p.key)).map(|sp|tn(sp.label)).unwrap_or_else(||p.key.clone()))))).collect();
        if options.is_empty(){ui.label(t("Animate a parameter with at least two keyframes first."));return;}
        if !options.iter().any(|x|x.0==s.curves.parameter){s.curves.parameter=options[0].0.clone();s.curves.segment=0;}
        egui::ComboBox::from_id_salt("curve-param").selected_text(options.iter().find(|x|x.0==s.curves.parameter).map(|x|x.3.clone()).unwrap_or_default()).show_ui(ui,|ui| {
            for (id,_,_,label) in &options{if ui.selectable_value(&mut s.curves.parameter,id.clone(),label).clicked(){s.curves.segment=0;}}
        });
        let (_,comp,prm,_)=options.iter().find(|x|x.0==s.curves.parameter).unwrap();
        s.curves.segment=s.curves.segment.min(prm.keys.len()-2);
        ui.horizontal(|ui| {
            ui.label(t("Segment"));ui.add(egui::DragValue::new(&mut s.curves.segment).range(0..=prm.keys.len()-2));
            ui.checkbox(&mut s.curves.speed,t("Speed Graph"));
            let axes=match prm.value{Value::Point(_)=>2,Value::Color(_)=>4,_=>1};
            if axes>1{egui::ComboBox::from_id_salt("curve-axis").selected_text(["X / R","Y / G","B","A"][s.curves.axis.min(axes-1)]).show_ui(ui,|ui|{for (i,label) in ["X / R","Y / G","B","A"].iter().enumerate().take(axes){ui.selectable_value(&mut s.curves.axis,i,*label);}});}
            s.curves.axis=s.curves.axis.min(axes-1);
        });
        let index=s.curves.segment;
        let (a,b)=(&prm.keys[index],&prm.keys[index+1]);
        let span=(b.time-a.time).seconds();
        let axis=s.curves.axis;
        let mut h=handles(a,b);
        let before=h;
        let (outer,_)=ui.allocate_exact_size(vec2(ui.available_width().max(240.0),230.0),Sense::hover());
        let r=outer.shrink2(vec2(28.0,16.0));
        let values:Vec<f64>=(0..=128).map(|i| {
            let at=a.time+Dur::from_seconds(span*i as f64/128.0);
            let value=scalar(&op_core::params::evaluate(&prm.keys,at),axis);
            if s.curves.speed {
                let dt=(span/1000.0).max(1e-6);
                let lo=(at-Dur::from_seconds(dt)).max(a.time);
                let hi=(at+Dur::from_seconds(dt)).min(b.time);
                (scalar(&op_core::params::evaluate(&prm.keys,hi),axis)-scalar(&op_core::params::evaluate(&prm.keys,lo),axis))/(hi-lo).seconds().max(1e-9)
            }else{value}
        }).collect();
        let low=values.iter().copied().fold(f64::INFINITY,f64::min);
        let high=values.iter().copied().fold(f64::NEG_INFINITY,f64::max);
        let margin=((high-low)*0.25).max(0.001);
        let (low,high)=s.curves.drag_range.unwrap_or((low-margin,high+margin));
        let point=|x:f64,y:f64|pos2(r.left()+r.width()*x as f32,r.bottom()-r.height()*((y-low)/(high-low)) as f32);
        let painter=ui.painter().with_clip_rect(outer);
        painter.rect_filled(r,4.0,theme::PANEL_DARK);
        for i in 0..=4 {let x=r.left()+r.width()*i as f32/4.0;let y=r.top()+r.height()*i as f32/4.0;painter.line_segment([pos2(x,r.top()),pos2(x,r.bottom())],Stroke::new(0.5,theme::TEXT_DIM));painter.line_segment([pos2(r.left(),y),pos2(r.right(),y)],Stroke::new(0.5,theme::TEXT_DIM));}
        let points:Vec<_>=values.iter().enumerate().map(|(i,v)|point(i as f64/128.0,*v)).collect();
        painter.add(egui::Shape::line(points,Stroke::new(2.0,theme::KEYFRAME)));
        ui.small(format!("{:.3} s → {:.3} s  |  {:.4} … {:.4}",a.time.seconds(),b.time.seconds(),low,high));
        let va=scalar(&a.value,axis);let vb=scalar(&b.value,axis);
        if !s.curves.speed && (vb-va).abs()>1e-12 {
            for j in 0..2 {
                let n=j*2;let hp=point(h[n],va+h[n+1]*(vb-va));
                painter.line_segment([point(j as f64,if j==0{va}else{vb}),hp],Stroke::new(1.0,Color32::LIGHT_BLUE));
                let resp=ui.interact(egui::Rect::from_center_size(hp,vec2(16.0,16.0)),ui.id().with(("curve-handle",&s.curves.parameter,index,j)),Sense::drag());
                painter.circle_filled(hp,5.0,Color32::LIGHT_BLUE);
                if resp.drag_started(){s.curves.drag_range=Some((low,high));}
                if resp.dragged() && let Some(p)=resp.interact_pointer_pos(){
                    h[n]=((p.x-r.left())/r.width()).clamp(0.001,0.999) as f64;
                    let value=low+(r.bottom()-p.y) as f64/r.height() as f64*(high-low);
                    h[n+1]=((value-va)/(vb-va)).clamp(-2.0,3.0);
                }
                if resp.drag_stopped(){s.curves.drag_range=None;s.ed.seal();}
            }
        }
        ui.horizontal(|ui| {
            for (i,label) in ["X1","Y1","X2","Y2"].iter().enumerate(){ui.label(*label);ui.add(egui::DragValue::new(&mut h[i]).speed(0.01).range(if i%2==0{0.001..=0.999}else{-2.0..=3.0}));}
        });
        ui.horizontal_wrapped(|ui| {for (label,handles) in BUILTIN_CURVES{if ui.button(t(label)).clicked(){h = *handles;}}});
        let saved=s.ed.prefs.plugins.curves.clone();
        if ui.button(t("Bounce")).clicked() {
            let (cid,key,at,end,keys)=(comp.id,prm.key.clone(),a.time,b.time,op_application::plugins::bounce(a,b));
            s.ed.edit("Add Bounce Keyframes",move|p| {
                let c=p.sequence_mut(sid).and_then(|s|s.clip_mut(clip)).and_then(|c|c.component_by_id_mut(cid)).ok_or(EditError::Nothing)?;
                let prm=c.param_mut(&key).ok_or(EditError::Nothing)?;
                if keys.windows(2).any(|k|k[0].time>=k[1].time){return Err(EditError::Nothing);}
                prm.keys.retain(|k|k.time<at||k.time>end);
                prm.keys.extend(keys);prm.sort_keys();Ok(())
            });
        }
        ui.horizontal_wrapped(|ui| {for preset in &saved{
            let response=ui.button(&preset.name);
            if response.clicked(){h=preset.handles;}
            response.context_menu(|ui|{if ui.button(t("Delete")).clicked(){s.ed.prefs.plugins.curves.retain(|p|p.name!=preset.name);let _=s.ed.prefs.save(&s.ed.dirs);ui.close();}});
        }});
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut s.curves.preset_name).hint_text(t("Curve preset name")));
            if ui.add_enabled(!s.curves.preset_name.trim().is_empty(),egui::Button::new(t("Save Preset"))).clicked(){
                let name=s.curves.preset_name.trim().to_string();
                s.ed.prefs.plugins.curves.retain(|p|p.name!=name);
                s.ed.prefs.plugins.curves.push(CurvePreset{name,handles:h});
                let _=s.ed.prefs.save(&s.ed.dirs);
                s.curves.preset_name.clear();
            }
        });
        if h!=before {
            let (cid,key,at,end)=(comp.id,prm.key.clone(),a.time,b.time);
            s.ed.edit_merge("Change Motion Curve",Some(format!("curve-{}-{}-{key}",clip.0,cid.0)),move|p|{
                let c=p.sequence_mut(sid).and_then(|s|s.clip_mut(clip)).and_then(|c|c.component_by_id_mut(cid)).ok_or(EditError::Nothing)?;
                let prm=c.param_mut(&key).ok_or(EditError::Nothing)?;
                let i=prm.key_at(at).ok_or(EditError::Nothing)?;
                if prm.keys.get(i+1).is_none_or(|k|k.time!=end){return Err(EditError::Nothing);}
                let (left,right)=prm.keys.split_at_mut(i+1);
                CurvePreset{name:String::new(),handles:h}.apply(&mut left[i],&mut right[0]);
                Ok(())
            });
        }
        ui.small(t("Drag blue handles or use presets. The graph shows the actual project animation; X is time, Y is value."));
    });
    s.curves.open = open;
}
