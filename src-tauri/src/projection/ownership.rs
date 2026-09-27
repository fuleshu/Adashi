//! Local ownership survives interrupted publication; canonical data never depends on this file.
use super::*;
use serde::{Deserialize,Serialize};
const MANIFEST:&str=".adashi/local/markdown-projection.json";

#[derive(Default,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest { files:BTreeMap<String,String> }

fn load(root:&Path)->Result<Manifest,String> {
    let path=files::checked(root,MANIFEST)?;
    match fs::read(path) {
        Ok(bytes)=> {
            let manifest:Manifest=serde_json::from_slice(&bytes).map_err(|e|format!("Invalid local projection ownership: {e}"))?;
            let mut normalized=Manifest::default();
            for (path,hash) in manifest.files {
                if normalized.files.insert(files::key(&path),hash).is_some() { return Err(format!("Ambiguous projection ownership: {path}")); }
            }
            Ok(normalized)
        },
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(Manifest::default()),
        Err(e)=>Err(e.to_string()),
    }
}

pub(super) fn status(root:&Path,plan:&markdown::Plan)->Result<Vec<ProjectionFileStatus>,String> {
    let owned=load(root)?;
    plan.files.iter().map(|(path,content)| {
        let target=files::checked(root,path)?;
        let key=files::key(path);
        let state=match fs::read(target) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>"missing",
            Err(e)=>return Err(e.to_string()),
            Ok(_) if !owned.files.contains_key(&key)=>"error",
            Ok(bytes) if bytes==content.as_bytes()=>"current",
            Ok(bytes) if owned.files.get(&key)==Some(&files::hash(&bytes))=>"stale",
            Ok(_)=>"drifted",
        };
        Ok(ProjectionFileStatus {path:path.clone(),state:state.into()})
    }).collect()
}

pub(super) fn publish(root:&Path,plan:&markdown::Plan,report:&mut ProjectionReport)->Result<(),String> {
    let lock_path=files::checked(root,".adashi/local/markdown-projection.lock")?;
    fs::create_dir_all(lock_path.parent().unwrap()).map_err(|e|e.to_string())?;
    let lock=fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(lock_path).map_err(|e|e.to_string())?;
    lock.try_lock().map_err(|e|format!("Projection busy; retry: {e}"))?;
    let mut owned=load(root)?;
    // Preflight the complete plan before claiming or touching any document.
    for path in plan.files.keys() {
        let target=files::checked(root,path)?;
        if target.exists() && !owned.files.contains_key(&files::key(path)) { return Err(format!("Unowned file blocks generated output: {path}")); }
    }
    for path in owned.files.keys() { files::checked(root,path)?; }
    for (path,content) in &plan.files {
        let key=files::key(path);
        let target=files::checked(root,path)?;
        let previous=fs::read(&target).ok();
        let hash=files::hash(content.as_bytes());
        if previous.as_ref().is_some_and(|b|owned.files.get(&key)!=Some(&files::hash(b))) {report.repaired.push(path.clone());}
        if !owned.files.contains_key(&key) {
            // Persist ownership before the atomic rename. An interrupted write remains retryable.
            owned.files.insert(key.clone(),hash.clone());
            files::write(root,MANIFEST,&serde_json::to_vec_pretty(&owned).map_err(|e|e.to_string())?)?;
        }
        if files::write(root,path,content.as_bytes())? {report.written.push(path.clone());}
        owned.files.insert(key,hash);
    }
    let expected=plan.files.keys().map(|path|files::key(path)).collect::<BTreeSet<_>>();
    for path in owned.files.keys().cloned().collect::<Vec<_>>() {
        if expected.contains(&path) {continue;}
        let target=files::checked(root,&path)?;
        let actual=relative_display(root,&target);
        match fs::remove_file(target) { Ok(())=>report.removed.push(actual),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string()) }
        owned.files.remove(&path);
    }
    files::write(root,MANIFEST,&serde_json::to_vec_pretty(&owned).map_err(|e|e.to_string())?)?;
    Ok(())
}
