import os
import json
import zipfile

SVG_ROOT = "tests/snapshots/svg"
SAMPLES_ROOT = "tests/samples"
OUTPUT_FILE = "preview.html"

def get_xml_path(svg_full_path):
    parts = svg_full_path.split(os.sep)
    try:
        svg_idx = parts.index('svg')
        sub_path = parts[svg_idx+1:]
        filename_base = sub_path[-1]
        if filename_base.endswith(".current.svg"):
            filename_base = filename_base[:-12]
        else:
            filename_base = filename_base[:-4]

        for ext in [".xml", ".mxl"]:
            candidate_filename = filename_base + ext
            sub_path[-1] = candidate_filename
            xml_rel_path = os.path.join(SAMPLES_ROOT, *sub_path)
            if os.path.exists(xml_rel_path):
                return xml_rel_path

        for root, dirs, files in os.walk(SAMPLES_ROOT):
            for ext in [".xml", ".mxl"]:
                if (filename_base + ext) in files:
                    return os.path.join(root, filename_base + ext)
    except:
        pass
    return None

def read_xml_content(xml_path):
    if xml_path.endswith(".mxl"):
        try:
            with zipfile.ZipFile(xml_path, 'r') as archive:
                # Try to find the root file
                xml_content = None
                if "META-INF/container.xml" in archive.namelist():
                    with archive.open("META-INF/container.xml") as f:
                        import xml.etree.ElementTree as ET
                        try:
                            tree = ET.parse(f)
                            root = tree.getroot()
                            # Try with and without namespace
                            rootfile = root.find(".//{urn:oasis:names:tc:opendocument:xmlns:container}rootfile")
                            if rootfile is None:
                                rootfile = root.find(".//rootfile")

                            if rootfile is not None:
                                full_path = rootfile.get("full-path")
                                if full_path in archive.namelist():
                                    with archive.open(full_path) as xf:
                                        xml_content = xf.read().decode('utf-8')
                        except Exception:
                            pass

                if xml_content is None:
                    # Fallback to first .xml/.musicxml
                    for name in archive.namelist():
                        if name.endswith(".xml") or name.endswith(".musicxml"):
                            with archive.open(name) as xf:
                                xml_content = xf.read().decode('utf-8')
                                break

                return xml_content if xml_content else f"[Compressed MXL file: {os.path.basename(xml_path)} - XML not found]"
        except Exception as e:
            return f"[Error reading MXL: {str(e)}]"

    try:
        with open(xml_path, "r", encoding="utf-8") as xf:
            return xf.read()
    except:
        return "Error reading XML file"


def get_file_tree(root_dir):
    tree = {}
    for root, dirs, files in os.walk(root_dir):
        rel_path = os.path.relpath(root, root_dir)
        if rel_path == ".":
            current_level = tree
        else:
            parts = rel_path.split(os.sep)
            current_level = tree
            for part in parts:
                if part not in current_level:
                    current_level[part] = {}
                current_level = current_level[part]

        svg_files = [f for f in files if f.endswith(".svg")]
        base_names = {}
        for f in svg_files:
            if f.endswith(".current.svg"):
                slug = f[:-12]
                base_names.setdefault(slug, {})["current"] = f
            else:
                slug = f[:-4]
                base_names.setdefault(slug, {})["base"] = f

        current_level["_files"] = []
        for slug in sorted(base_names.keys()):
            versions = base_names[slug]
            path_prefix = rel_path if rel_path != "." else ""

            actual_file = versions.get("current", versions.get("base"))
            full_path = os.path.join(SVG_ROOT, path_prefix, actual_file)

            xml_path = get_xml_path(full_path)
            xml_content = read_xml_content(xml_path) if xml_path else ""

            current_level["_files"].append({
                "name": slug,
                "path": full_path,
                "has_current": "current" in versions,
                "xml": xml_content
            })

    return tree

def generate_preview():
    tree = get_file_tree(SVG_ROOT)
    tree_json = json.dumps(tree)

    html = f"""
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>staveloom SVG Preview</title>
        <style>
            body {{
                font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
                margin: 0;
                display: flex;
                height: 100vh;
                overflow: hidden;
                background-color: #f5f5f5;
            }}
            #sidebar {{
                width: 300px;
                background-color: #fff;
                border-right: 1px solid #ddd;
                overflow-y: auto;
                padding: 20px;
                flex-shrink: 0;
                z-index: 10;
            }}
            #content-wrapper {{
                flex-grow: 1;
                display: flex;
                position: relative;
                overflow: hidden;
            }}
            #content {{
                flex-grow: 1;
                overflow-y: auto;
                padding: 40px;
                display: flex;
                flex-direction: column;
                align-items: center;
                transition: margin-right 0.3s ease;
            }}
            #xml-panel {{
                position: absolute;
                right: -50%;
                top: 0;
                width: 50%;
                height: 100%;
                background-color: #282c34;
                color: #abb2bf;
                border-left: 1px solid #181a1f;
                transition: right 0.3s ease;
                display: flex;
                flex-direction: column;
                box-shadow: -5px 0 15px rgba(0,0,0,0.2);
                z-index: 20;
            }}
            #xml-panel.open {{
                right: 0;
            }}
            #xml-header {{
                padding: 15px 20px;
                background-color: #21252b;
                display: flex;
                justify-content: space-between;
                align-items: center;
                border-bottom: 1px solid #181a1f;
            }}
            #xml-content {{
                flex-grow: 1;
                overflow: auto;
                padding: 20px;
                margin: 0;
                font-family: 'Courier New', Courier, monospace;
                font-size: 13px;
                line-height: 1.5;
                white-space: pre;
            }}
            .close-btn {{
                cursor: pointer;
                background: none;
                border: none;
                color: #fff;
                font-size: 20px;
            }}
            .toggle-xml-btn {{
                position: fixed;
                right: 20px;
                top: 20px;
                padding: 8px 15px;
                background-color: #007bff;
                color: white;
                border: none;
                border-radius: 4px;
                cursor: pointer;
                box-shadow: 0 2px 10px rgba(0,0,0,0.2);
                z-index: 15;
            }}
            .folder {{ margin-left: 15px; margin-top: 5px; }}
            .folder-name {{ font-weight: bold; cursor: pointer; color: #333; display: flex; align-items: center; }}
            .folder-name::before {{ content: '▶'; display: inline-block; margin-right: 5px; font-size: 10px; transition: transform 0.2s; }}
            .folder.open > .folder-name::before {{ transform: rotate(90deg); }}
            .folder-content {{ display: none; }}
            .folder.open > .folder-content {{ display: block; }}
            .file-item {{ padding: 4px 8px; cursor: pointer; border-radius: 4px; font-size: 14px; color: #666; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
            .file-item:hover {{ background-color: #f0f7ff; color: #007bff; }}
            .file-item.active {{ background-color: #007bff; color: #fff; }}
            .badge-current {{ font-size: 10px; background-color: #28a745; color: white; padding: 1px 4px; border-radius: 3px; margin-left: 5px; }}
            #viewer {{ background-color: #fff; box-shadow: 0 4px 20px rgba(0,0,0,0.1); padding: 20px; max-width: 100%; min-width: 400px; }}
            #viewer img {{ max-width: 100%; height: auto; }}
            #file-info {{ margin-bottom: 20px; text-align: center; }}
            h1 {{ font-size: 18px; margin: 0; }}
            .path-info {{ color: #888; font-size: 12px; margin-top: 5px; }}
        </style>
    </head>
    <body>
        <div id="sidebar">
            <h2 style="font-size: 20px; margin-bottom: 20px;">staveloom Preview</h2>
            <div id="tree-container"></div>
        </div>

        <div id="content-wrapper">
            <button id="toggle-btn" class="toggle-xml-btn" style="display:none;" onclick="toggleXml()">Show MusicXML</button>

            <div id="content">
                <div id="file-info">
                    <h1 id="current-filename">Select a file to preview</h1>
                    <div id="current-path" class="path-info"></div>
                </div>
                <div id="viewer">
                    <img id="preview-img" style="display:none;">
                </div>
            </div>

            <div id="xml-panel">
                <div id="xml-header">
                    <span style="font-weight: bold;">Source MusicXML</span>
                    <button class="close-btn" onclick="toggleXml()">&times;</button>
                </div>
                <pre id="xml-content"></pre>
            </div>
        </div>

        <script>
            const treeData = {tree_json};
            let currentXml = "";

            function renderTree(data, container) {{
                if (data._files) {{
                    data._files.forEach(file => {{
                        const div = document.createElement('div');
                        div.className = 'file-item';
                        div.innerHTML = file.name;
                        if (file.has_current) {{
                            div.innerHTML += '<span class="badge-current">new</span>';
                        }}
                        div.onclick = (e) => {{
                            e.stopPropagation();
                            selectFile(file, div);
                        }};
                        container.appendChild(div);
                    }});
                }}

                Object.keys(data).forEach(key => {{
                    if (key === '_files') return;
                    const folderDiv = document.createElement('div');
                    folderDiv.className = 'folder';
                    const nameDiv = document.createElement('div');
                    nameDiv.className = 'folder-name';
                    nameDiv.innerText = key;
                    nameDiv.onclick = () => folderDiv.classList.toggle('open');
                    const contentDiv = document.createElement('div');
                    contentDiv.className = 'folder-content';
                    folderDiv.appendChild(nameDiv);
                    folderDiv.appendChild(contentDiv);
                    container.appendChild(folderDiv);
                    renderTree(data[key], contentDiv);
                }});
            }}

            function selectFile(file, element) {{
                document.querySelectorAll('.file-item').forEach(el => el.classList.remove('active'));
                element.classList.add('active');

                document.getElementById('current-filename').innerText = file.name;
                document.getElementById('current-path').innerText = file.path;

                const img = document.getElementById('preview-img');
                img.src = file.path + '?t=' + new Date().getTime();
                img.style.display = 'block';

                // Scroll content to top
                document.getElementById('content').scrollTop = 0;

                // Update XML
                currentXml = file.xml;
                document.getElementById('xml-content').textContent = file.xml;
                document.getElementById('toggle-btn').style.display = file.xml ? 'block' : 'none';
            }}

            function toggleXml() {{
                const panel = document.getElementById('xml-panel');
                const btn = document.getElementById('toggle-btn');
                panel.classList.toggle('open');
                if (panel.classList.contains('open')) {{
                    btn.innerText = 'Hide MusicXML';
                }} else {{
                    btn.innerText = 'Show MusicXML';
                }}
            }}

            const container = document.getElementById('tree-container');
            renderTree(treeData, container);

            const folders = document.querySelectorAll('.folder');
            folders.forEach(f => {{
                if (f.querySelector('.folder-name').innerText === 'specification') {{
                    f.classList.add('open');
                }}
            }});
        </script>
    </body>
    </html>
    """

    with open(OUTPUT_FILE, "w", encoding="utf-8") as f:
        f.write(html)
    print(f"Preview page generated: {OUTPUT_FILE}")

if __name__ == "__main__":
    generate_preview()
