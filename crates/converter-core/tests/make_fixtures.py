"""Genera los fixtures de converter-core con codificadores independientes (Pillow, zipfile).

Uso (desde la raíz del repo): python -I crates/converter-core/tests/make_fixtures.py crates/converter-core/tests/fixtures
Requiere Pillow. Los fixtures generados se versionan; solo hay que re-ejecutarlo si cambian.
"""
import sys
import zipfile
from pathlib import Path

from PIL import Image

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)

W, H = 64, 48
img = Image.new("RGBA", (W, H))
px = img.load()
for y in range(H):
    for x in range(W):
        px[x, y] = (x * 4 % 256, y * 5 % 256, (x + y) * 2 % 256, 255)
# Esquina superior izquierda totalmente transparente: sirve para probar PNG -> JPG.
for y in range(10):
    for x in range(10):
        px[x, y] = (255, 0, 0, 0)

img.save(out / "sample_alpha.png")
img.save(out / "sample_alpha.webp", lossless=True)
img.save(out / "sample_alpha.tif")
rgb = img.convert("RGB")
rgb.save(out / "sample.png")
rgb.save(out / "sample.jpg", quality=92)
rgb.save(out / "sample.bmp")
rgb.save(out / "sample.tif")
rgb.save(out / "sample.webp", quality=90)
rgb.convert("P", palette=Image.ADAPTIVE, colors=64).save(out / "sample.gif")

# Foto "tumbada" con etiqueta EXIF de orientación 6 (girar 90° a la derecha): se ve 48x64.
exif = Image.Exif()
exif[0x0112] = 6
rgb.save(out / "rotated_exif.jpg", quality=92, exif=exif)
# JPG en escala de grises: se incrusta en PDF con DeviceGray.
rgb.convert("L").save(out / "gray.jpg", quality=92)


def make_text_pdf(path, pages):
    """PDF escrito a mano (sin librerías): una página por (ancho, alto, [líneas de texto]).

    Fuente Helvetica con WinAnsiEncoding, para que ñ y acentos tengan texto extraíble.
    """
    objects = {}  # número -> bytes del cuerpo

    def esc(s):
        return s.encode("cp1252").replace(b"\\", b"\\\\").replace(b"(", b"\\(").replace(b")", b"\\)")

    kids = []
    next_id = 4
    for width, height, lines in pages:
        page_id, content_id = next_id, next_id + 1
        next_id += 2
        kids.append(page_id)
        ops = [b"BT", b"/F1 24 Tf", b"72 %d Td" % (height - 100)]
        for i, line in enumerate(lines):
            if i:
                ops.append(b"0 -36 Td")
            ops.append(b"(" + esc(line) + b") Tj")
        ops.append(b"ET")
        # Un rectángulo negro: así la página renderizada tiene píxeles oscuros aunque no se lea el texto.
        ops.append(b"0 0 0 rg 72 72 200 40 re f")
        stream = b"\n".join(ops)
        objects[content_id] = b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream"
        objects[page_id] = (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] /Contents %d 0 R "
            b"/Resources << /Font << /F1 3 0 R >> >> >>" % (width, height, content_id)
        )
    objects[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objects[2] = b"<< /Type /Pages /Kids [" + b" ".join(b"%d 0 R" % k for k in kids) + b"] /Count %d >>" % len(kids)
    objects[3] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"

    body = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for number in sorted(objects):
        offsets[number] = len(body)
        body += b"%d 0 obj\n" % number + objects[number] + b"\nendobj\n"
    xref = len(body)
    body += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for number in sorted(objects):
        body += b"%010d 00000 n \n" % offsets[number]
    body += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)
    Path(path).write_bytes(bytes(body))


make_text_pdf(
    out / "sample.pdf",
    [
        (595, 842, ["Página 1", "Hola mundo, ñandú y café"]),
        (842, 595, ["Página 2 (apaisada)", "Segunda hoja"]),
        (595, 842, ["Página 3", "Fin del documento"]),
    ],
)
def make_structured_pdf(path):
    """PDF escrito a mano con estructura reconocible: título, párrafos, negrita, subtítulo, viñetas,
    lista numerada, código, un párrafo que cruza de página y encabezado/pie repetidos."""

    def esc(text):
        return text.encode("cp1252").replace(b"\\", b"\\\\").replace(b"(", b"\\(").replace(b")", b"\\)")

    def line(font, size, x, y, *runs):
        """Una línea de texto; `runs` = (fuente, tamaño, texto)* tras el primero para cambiar de estilo."""
        out = [b"BT", b"/%s %g Tf" % (font.encode(), size), b"%g %g Td" % (x, y)]
        first = True
        for run in runs:
            if isinstance(run, tuple):
                out.append(b"/%s %g Tf" % (run[0].encode(), run[1]))
                out.append(b"(" + esc(run[2]) + b") Tj")
            else:
                out.append(b"(" + esc(run) + b") Tj")
        out.append(b"ET")
        return b"\n".join(out)

    def furniture(n):
        return [line("F1", 9, 72, 810, "Informe interno"), line("F1", 9, 260, 30, "Página %d" % n)]

    page1 = furniture(1) + [
        line("F2", 24, 72, 740, "Manual de usuario"),
        line("F1", 11, 72, 700, "Este manual explica la docu-"),
        line("F1", 11, 72, 686, "mentación del programa y sus"),
        line("F1", 11, 72, 672, "funciones principales."),
        line("F1", 11, 72, 640, "Lee con cuidado ", ("F2", 11, "importante"), ("F1", 11, " antes de empezar.")),
        line("F2", 16, 72, 600, "Instalación"),
        line("F1", 11, 72, 570, "• Descargar el instalador"),
        line("F1", 11, 72, 556, "• Ejecutarlo y aceptar"),
        line("F1", 11, 72, 530, "1. Abrir la aplicación"),
        line("F1", 11, 72, 516, "2. Pulsar el atajo"),
        line("F3", 10, 72, 480, "cargo build --release"),
        line("F3", 10, 84, 468, "converter.exe"),
        line("F1", 11, 72, 120, "Esta frase termina en la página"),
    ]
    page2 = furniture(2) + [
        line("F1", 11, 72, 760, "siguiente sin cortar el párrafo."),
        line("F2", 16, 72, 700, "Notas"),
        line("F1", 11, 72, 670, "Texto final de la segunda página."),
    ]
    objects = {}
    kids = []
    next_id = 6
    for ops in (page1, page2):
        page_id, content_id = next_id, next_id + 1
        next_id += 2
        kids.append(page_id)
        stream = b"\n".join(ops)
        objects[content_id] = b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream"
        objects[page_id] = (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents %d 0 R "
            b"/Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> >>" % content_id
        )
    objects[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objects[2] = b"<< /Type /Pages /Kids [" + b" ".join(b"%d 0 R" % k for k in kids) + b"] /Count %d >>" % len(kids)
    for number, name in ((3, "Helvetica"), (4, "Helvetica-Bold"), (5, "Courier")):
        objects[number] = b"<< /Type /Font /Subtype /Type1 /BaseFont /%s /Encoding /WinAnsiEncoding >>" % name.encode()
    body = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for number in sorted(objects):
        offsets[number] = len(body)
        body += b"%d 0 obj\n" % number + objects[number] + b"\nendobj\n"
    xref = len(body)
    body += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for number in sorted(objects):
        body += b"%010d 00000 n \n" % offsets[number]
    body += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)
    Path(path).write_bytes(bytes(body))


make_structured_pdf(out / "structured.pdf")

# Markdown de prueba: título, énfasis, listas, tabla, código, enlace e imagen remota (que no debe descargarse).
(out / "sample.md").write_text(
    "# Informe de prueba\n\n"
    "Hola mundo, **ñandú** y *café*: acentuación correcta.\n\n"
    "## Lista\n\n- uno\n- dos\n\n"
    "| Producto | Precio |\n|---|---|\n| Café | 2.500 |\n\n"
    "```\ncargo build\n```\n\n"
    "Más en [la web](https://ejemplo.com). ![logo](http://127.0.0.1:9/x.png)\n",
    encoding="utf-8",
)

# PDF "escaneado": una imagen y nada de texto, así que no tiene texto que extraer.
rgb.save(out / "scan.pdf")

# Imagen "fotográfica" (degradado + ruido con semilla fija) para comparar tamaños de WEBP/JPG.
import random

random.seed(7)
photo = Image.new("RGB", (640, 480))
ppx = photo.load()
for y in range(480):
    for x in range(640):
        n = random.randint(-18, 18)
        ppx[x, y] = (
            max(0, min(255, x * 255 // 640 + n)),
            max(0, min(255, y * 255 // 480 + n)),
            max(0, min(255, (x + y) * 255 // 1120 + n)),
        )
photo.save(out / "photo.jpg", quality=90)
photo.save(out / "photo.png")

# CSV con ';' (Excel en español), BOM, acentos, salto de línea entre comillas y ceros a la izquierda.
(out / "es_semicolon.csv").write_bytes(
    "﻿nombre;código;importe;nota\r\n"
    'Ñandú;007;1234.5;"línea 1\nlínea 2"\r\n'
    "José;12;-3;\r\n".encode("utf-8")
)
(out / "plain.csv").write_text("a,b,c\n1,2.5,hola\n3,,\"x,y\"\n", encoding="utf-8")

# XLSX escrito a mano (sin Excel ni openpyxl): dos hojas, número, texto, vacío y booleano.
content_types = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
    '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
    '<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
    "</Types>"
)
rels = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
    "</Relationships>"
)
workbook = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
    '<sheets><sheet name="Ventas" sheetId="1" r:id="rId1"/><sheet name="Otra" sheetId="2" r:id="rId2"/></sheets>'
    "</workbook>"
)
wb_rels = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
    '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>'
    "</Relationships>"
)


def inline(ref, text):
    return f'<c r="{ref}" t="inlineStr"><is><t>{text}</t></is></c>'


sheet1 = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>'
    f'<row r="1">{inline("A1", "producto")}{inline("B1", "unidades")}{inline("C1", "precio")}{inline("D1", "activo")}</row>'
    f'<row r="2">{inline("A2", "Café")}<c r="B2"><v>3</v></c><c r="C2"><v>2.5</v></c><c r="D2" t="b"><v>1</v></c></row>'
    f'<row r="3">{inline("A3", "Té")}<c r="B3"><v>10</v></c><c r="D3" t="b"><v>0</v></c></row>'
    "</sheetData></worksheet>"
)
sheet2 = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>'
    f'<row r="1">{inline("A1", "solo en la segunda hoja")}</row>'
    "</sheetData></worksheet>"
)
with zipfile.ZipFile(out / "sample.xlsx", "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", content_types)
    z.writestr("_rels/.rels", rels)
    z.writestr("xl/workbook.xml", workbook)
    z.writestr("xl/_rels/workbook.xml.rels", wb_rels)
    z.writestr("xl/worksheets/sheet1.xml", sheet1)
    z.writestr("xl/worksheets/sheet2.xml", sheet2)

# DOCX escrito a mano (sin Word ni librerías): títulos, acentos, tabulador, tabla y un párrafo final.
W_NS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"


def para(text, bold=False):
    rpr = "<w:rPr><w:b/></w:rPr>" if bold else ""
    return f'<w:p><w:r>{rpr}<w:t xml:space="preserve">{text}</w:t></w:r></w:p>'


def cell(text):
    return f"<w:tc><w:tcPr><w:tcW w:w=\"2400\" w:type=\"dxa\"/></w:tcPr>{para(text)}</w:tc>"


document_xml = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    f'<w:document xmlns:w="{W_NS}"><w:body>'
    + para("Informe de prueba", bold=True)
    + para("Hola mundo, ñandú y café: acentuación correcta.")
    + '<w:p><w:r><w:t>uno</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>dos</w:t></w:r></w:p>'
    + '<w:tbl><w:tblPr><w:tblBorders>'
    + "".join(f'<w:{side} w:val="single" w:sz="4" w:color="000000"/>' for side in ("top", "left", "bottom", "right", "insideH", "insideV"))
    + "</w:tblBorders></w:tblPr>"
    + '<w:tblGrid><w:gridCol w:w="2400"/><w:gridCol w:w="2400"/></w:tblGrid>'
    + f"<w:tr>{cell('Producto')}{cell('Precio')}</w:tr>"
    + f"<w:tr>{cell('Café')}{cell('2.500')}</w:tr>"
    + "</w:tbl>"
    + para("Fin del documento.")
    + '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>'
    "</w:body></w:document>"
)
with zipfile.ZipFile(out / "sample.docx", "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr(
        "[Content_Types].xml",
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
        "</Types>",
    )
    z.writestr(
        "_rels/.rels",
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
        "</Relationships>",
    )
    z.writestr("word/document.xml", document_xml)

(out / "records.json").write_text(
    '[{"id":1,"name":"Ana","tags":["a","b"]},{"id":2,"name":"Luis","vip":true},{"id":3,"name":null}]',
    encoding="utf-8",
)
print("fixtures en", out)
