// Folder of STEP → STL
// @param dir Folder | .
// @param out_dir Output folder | export
let files = list_dir(dir);
for path in files {
    let p = path;
    if p.contains(".step") || p.contains(".stp") || p.contains(".STEP") || p.contains(".STP") {
        run("doc.open", #{ path: p });
        let stem = file_stem(p);
        run("file.export", #{ path: out_dir + "/" + stem + ".stl" });
    }
}
