// Folder of STEP → STL
// @param dir Folder | .
// @param out_dir Output folder | export
let files = list_dir(dir);
for path in files {
    let upper = path.to_upper();
    if upper.ends_with(".STEP") || upper.ends_with(".STP") {
        run("doc.open", #{ path: path });
        let stem = file_stem(path);
        run("file.export", #{ path: out_dir + "/" + stem + ".stl" });
    }
}
