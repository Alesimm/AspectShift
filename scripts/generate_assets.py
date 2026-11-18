"""
Genera los recursos gráficos de AspectShift en múltiples resoluciones.
Renderiza a 1024x1024 y reduce con LANCZOS para lograr bordes definidos y suaves.
"""

import os
from PIL import Image, ImageDraw, ImageFilter


def crear_mascara_squircle(tamano: int, radio: int) -> Image.Image:
    mascara = Image.new("L", (tamano, tamano), 0)
    dibujo = ImageDraw.Draw(mascara)
    dibujo.rounded_rectangle((28, 28, tamano - 29, tamano - 29), radius=radio, fill=255)
    return mascara


def renderizar_icono_maestro(tamano: int = 1024) -> Image.Image:
    lienzo = Image.new("RGBA", (tamano, tamano), (0, 0, 0, 0))

    # Fondo grafito suave con degradado vertical sutil
    fondo = Image.new("RGBA", (tamano, tamano), (0, 0, 0, 0))
    pixeles = fondo.load()
    for y in range(tamano):
        t = y / float(tamano - 1)
        r = int(18 * (1.0 - t) + 28 * t)
        g = int(21 * (1.0 - t) + 32 * t)
        b = int(28 * (1.0 - t) + 42 * t)
        for x in range(tamano):
            pixeles[x, y] = (r, g, b, 255)

    mascara = crear_mascara_squircle(tamano, radio=216)
    lienzo.paste(fondo, (0, 0), mascara)

    # Borde exterior suave en tono pizarra e iluminación sutil superior
    dibujo = ImageDraw.Draw(lienzo)
    dibujo.rounded_rectangle(
        (28, 28, tamano - 29, tamano - 29),
        radius=216,
        outline=(54, 64, 84, 240),
        width=10,
    )
    dibujo.rounded_rectangle(
        (42, 42, tamano - 43, tamano - 43),
        radius=202,
        outline=(255, 255, 255, 22),
        width=4,
    )

    # Halo suave índigo detrás del módulo central
    resplandor = Image.new("RGBA", (tamano, tamano), (0, 0, 0, 0))
    dib_res = ImageDraw.Draw(resplandor)
    dib_res.rounded_rectangle(
        (240, 260, tamano - 240, tamano - 260),
        radius=90,
        fill=(110, 142, 247, 52),
    )
    resplandor = resplandor.filter(ImageFilter.GaussianBlur(radius=64))
    lienzo = Image.alpha_composite(lienzo, resplandor)
    dibujo = ImageDraw.Draw(lienzo)

    # Marco posterior 16:9 (panorámico base)
    ext_izq, ext_arr = 148, 292
    ext_der, ext_aba = tamano - 148, tamano - 292
    dibujo.rounded_rectangle(
        (ext_izq, ext_arr, ext_der, ext_aba),
        radius=48,
        fill=(26, 31, 43, 215),
        outline=(84, 102, 138, 210),
        width=12,
    )

    # Líneas sutiles de proporción lateral en el marco 16:9
    centro_y = tamano // 2
    dibujo.line([(186, centro_y - 36), (186, centro_y + 36)], fill=(120, 140, 178, 175), width=10)
    dibujo.line(
        [(tamano - 186, centro_y - 36), (tamano - 186, centro_y + 36)],
        fill=(120, 140, 178, 175),
        width=10,
    )

    # Tarjeta frontal 4:3 en azul índigo suave (#6E8EF7)
    capa_frente = Image.new("RGBA", (tamano, tamano), (0, 0, 0, 0))
    dib_frente = ImageDraw.Draw(capa_frente)
    in_izq, in_arr = 248, 220
    in_der, in_aba = tamano - 248, tamano - 220

    dib_frente.rounded_rectangle(
        (in_izq, in_arr, in_der, in_aba),
        radius=54,
        fill=(40, 52, 82, 235),
        outline=(110, 142, 247, 255),
        width=18,
    )

    # Sombra/relieve suave de la tarjeta frontal
    brillo_frente = capa_frente.filter(ImageFilter.GaussianBlur(radius=18))
    lienzo = Image.alpha_composite(lienzo, brillo_frente)
    lienzo = Image.alpha_composite(lienzo, capa_frente)
    dibujo = ImageDraw.Draw(lienzo)

    # Detalles geométricos minimalistas dentro del marco 4:3
    # Barra superior limpia representando cabecera de ventana / relación activa
    dibujo.rounded_rectangle(
        (in_izq + 56, in_arr + 56, in_der - 150, in_arr + 88),
        radius=16,
        fill=(236, 240, 252, 225),
    )
    # Punto verde salvia (#56B69F) indicando estado activo
    dibujo.ellipse(
        (in_der - 110, in_arr + 50, in_der - 66, in_arr + 94),
        fill=(86, 182, 159, 255),
    )

    # Indicador central minimalista de transición horizontal (dos trazos paralelos limpios)
    cx = tamano // 2
    cy = centro_y + 26
    dibujo.rounded_rectangle(
        (cx - 126, cy - 10, cx + 126, cy + 10),
        radius=10,
        fill=(142, 168, 250, 235),
    )
    dibujo.rounded_rectangle(
        (cx - 72, cy + 36, cx + 72, cy + 56),
        radius=10,
        fill=(86, 182, 159, 220),
    )

    return lienzo


def main():
    raiz = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    carpeta_assets = os.path.join(raiz, "assets")
    os.makedirs(carpeta_assets, exist_ok=True)

    maestro = renderizar_icono_maestro(1024)

    # Logo PNG de 256x256 para documentación e interfaz
    logo_256 = maestro.resize((256, 256), Image.Resampling.LANCZOS)
    ruta_logo = os.path.join(carpeta_assets, "logo.png")
    logo_256.save(ruta_logo, format="PNG", optimize=True)

    # Archivo .ico multi-resolución para Windows
    tamanos_ico = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    ruta_ico = os.path.join(carpeta_assets, "icon.ico")
    maestro.save(ruta_ico, format="ICO", sizes=tamanos_ico)

    # Búfer RGBA 64x64 embebido directamente en el binario
    icono_64 = maestro.resize((64, 64), Image.Resampling.LANCZOS)
    ruta_rgba = os.path.join(carpeta_assets, "icon_rgba.bin")
    with open(ruta_rgba, "wb") as archivo:
        archivo.write(icono_64.tobytes("raw", "RGBA"))

    print(f"Generado {ruta_ico} ({os.path.getsize(ruta_ico)} bytes)")
    print(f"Generado {ruta_logo} ({os.path.getsize(ruta_logo)} bytes)")
    print(f"Generado {ruta_rgba} ({os.path.getsize(ruta_rgba)} bytes)")


if __name__ == "__main__":
    main()
