import os

XML_DIR = "tests/samples/specification/xml"
IMG_DIR = "tests/samples/specification/img"
SVG_DIR = "tests/snapshots/svg/specification/xml"
REPORT_FILE = "specification_report.html"


def generate_report():
    html = """
    <html>
    <head>
        <title>MusicXML Rendering Report</title>
        <style>
            table { width: 100%; border-collapse: collapse; }
            th, td { border: 1px solid #ccc; padding: 10px; text-align: left; vertical-align: top; }
            .slug-col { width: 80px; font-size: 11px; word-break: break-all; }
            .xml-col pre { background: #f4f4f4; padding: 5px; font-size: 11px; max-width: 300px; overflow: auto; max-height: 300px;}
            .ref-col img { max-width: 250px; height: auto; border: 1px solid #eee; }
            .svg-col img { height: auto; border: 1px solid #eee; } /* No max-width for original scale */
            .fail { color: red; font-weight: bold; }
        </style>
    </head>
    <body>
        <h1>MusicXML Specification Rendering Report</h1>
        <table>
            <tr>
                <th class="slug-col">Element / File</th>
                <th>Source XML</th>
                <th>W3C Reference (PNG)</th>
                <th>Rendered Result (SVG)</th>
            </tr>
    """

    # Only include items that have BOTH XML and Reference Image
    files = sorted([f for f in os.listdir(XML_DIR) if f.endswith(".xml")])
    valid_slugs = []
    for filename in files:
        slug = os.path.splitext(filename)[0]
        if os.path.exists(os.path.join(IMG_DIR, f"{slug}.png")):
            valid_slugs.append(slug)

    for slug in valid_slugs:
        xml_path = os.path.join(XML_DIR, f"{slug}.xml")
        img_path = os.path.join(IMG_DIR, f"{slug}.png")

        # Check for current SVG first, then baseline SVG
        svg_path = os.path.join(SVG_DIR, f"{slug}.current.svg")
        if not os.path.exists(svg_path):
            svg_path = os.path.join(SVG_DIR, f"{slug}.svg")

        html += f"<tr>"
        html += f"<td class='slug-col'><strong>{slug}</strong></td>"

        with open(xml_path, "r") as f:
            xml_content = f.read()
        html += (
            f"<td class='xml-col'><pre>{xml_content.replace('<', '&lt;')}</pre></td>"
        )

        # Reference Image
        html += f"<td class='ref-col'><img src='{img_path}' alt='Reference'></td>"

        # Rendered SVG
        if os.path.exists(svg_path):
            html += f"<td class='svg-col'><img src='{svg_path}' alt='Rendered'></td>"
        else:
            html += "<td><span class='fail'>Not Rendered</span></td>"

        html += "</tr>"

    html += """
        </table>
    </body>
    </html>
    """

    with open(REPORT_FILE, "w") as f:
        f.write(html)
    print(f"Report generated: {REPORT_FILE}")


if __name__ == "__main__":
    generate_report()
