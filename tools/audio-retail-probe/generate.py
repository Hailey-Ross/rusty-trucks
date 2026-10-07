"""Build private vectors from owned TU3 instruction translations, never from Rust audio code."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

IMAGE_SHA = "f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4"
FUNCTIONS = {
    "gain": ["82B3C098"],
    "jump": ["82772D30"],
    "offboard": ["824B0DA8"],
    "speech_queue": ["82971890"],
    "speech_levels": ["824D9370", "824DA300"],
    "speech_interrupt": ["824A73F0", "824A62F0"],
    "bail_contact": ["82BD60C8"],
    "speech_queue_lifecycle": ["82971340", "82971DA8", "82971890", "82971C78"],
    "ambience": ["824D3C28", "824D3FE0", "824D41E0"],
    "mixmap_full": ['8294AF70', '8294B000', '8294B040', '8294B1FC', '8294B2A4', '8294B2E4', '8294B304', '8294B324', '8294B344', '8294B4A0', '8294B4D8', '8294B5F8', '8294B668', '8294B6B4', '8294B6C8', '8294B724', '8294B744', '8294B794', '8294B7B4', '8294B7D0', '8294B7EC', '8294B808', '8294B824', '8294B858', '8294B8B0', '8294B918', '8294BA18', '8294BAE8', '8294BB50', '8294BBE8', '8294BBF0', '8294BC50', '8294BC68', '8294BCA0', '8294BCC0', '8294BD98', '8294BDB0', '8294BDC8', '8294BF40', '8294C0C0', '8294C190', '8294C348', '8294C430', '8294C500', '8294C548', '8294CB48', '8294CC48', '8294CC90', '8294CDE8', '8294D438', '8294DD20', '8294DE10', '8294DF08', '8294E008', '8294E120', '8294E970', '8294EDF8', '8294F228', '8294F528', '8294F5E8', '8294F65C', '8294F6EC', '8294F704', '8294F71C', '8294F734', '8294F74C', '8294F764', '8294F778', '8294F794', '8294F7B0', '8294F7CC', '8294F7E8', '8294F804', '8294F820', '8294F83C', '8294F858', '8294F870', '8294F924', '8294F93C', '8294F954', '8294F96C', '8294F984', '8294F99C', '8294F9B0', '8294F9CC', '8294F9E8', '8294FA04', '8294FA20', '8294FA3C', '8294FA58', '8294FA74', '8294FA90', '8294FAA8', '8294FC88', '8294FCD4', '8294FD14', '8294FE6C', '8294FE80', '8294FE98', '8294FEB0', '8294FEC8', '8294FEE0', '8294FEF8', '8294FF10', '8294FF28', '8294FF40', '8294FF58', '8294FF70', '8294FF88', '8294FFA0', '829500F4', '8295010C', '82950198', '82950250', '829502A4', '8295041C', '82950440', '82950464', '829506D0', '82950754', '8295076C', '82950784', '8295079C', '829507B4', '829507C8', '829508A8', '829508C0', '829508C4', '82950D40', '82951148', '82951440', '82951658', '8295177C', '82951A24', '82951A3C', '82951A54', '82951A6C', '82951A84', '82951A9C', '82951AB0', '82951ACC', '82951AE8', '82951B04', '82951B20', '82951B3C', '82951B58', '82951B74', '82951B90', '82951BA8', '82951D58', '82951DA0', '82951E00', '82951E80', '82952090', '829520AC', '829520CC', '829520EC', '8295210C', '8295212C', '8295222C', '82952314', '82952328', '82952340', '82952358', '82952370', '82952388', '829523A0', '829523B8', '829523D0', '829523E8', '8295248C', '82952498', '829524F0', '82952680', '829528A0', '8295292C', '82952B44', '82952BE4', '82952BF8', '82952C10', '82952C28', '82952C40', '82952C58', '82952C70', '82952C88', '82952CA0', '82952CB8', '82952CE8', '82952D00', '82952D18', '82952D44', '82952DD8', '82953080', '829531F8', '828DE848', '825E7458'],
    "mixmap_input": ["8294F5E8", "8294FC88", "82950250", "82951658", "8294B668"],
}


def main():
    p = argparse.ArgumentParser(__doc__)
    p.add_argument("kind", choices=FUNCTIONS)
    p.add_argument("--generated", type=Path, required=True)
    p.add_argument("--sdk", type=Path, required=True)
    p.add_argument("--image", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--clang", default="clang++")
    p.add_argument("--mxb", type=Path, help="owned MixMapSK8.mxb; required for mixmap_full")
    a = p.parse_args()
    digest = hashlib.sha256(a.image.read_bytes()).hexdigest()
    if digest != IMAGE_SHA:
        p.error("wrong TU3 mapped image; refusing to execute mismatched instructions/constants")
    if a.kind == "mixmap_full" and a.mxb is None:
        p.error("mixmap_full requires --mxb")
    wanted = set(FUNCTIONS[a.kind])
    bodies, provenance = {}, {}
    for source in sorted(a.generated.glob("skate3_recomp.*.cpp")):
        text = source.read_text(encoding="utf-8")
        for match in re.finditer(r"DEFINE_REX_FUNC\(sub_([0-9A-F]+)\) \{.*?\n\}", text, re.S):
            address = match[1]
            if address not in wanted:
                continue
            if address in bodies:
                raise ValueError("duplicate native function " + address)
            bodies[address] = match.group()
            provenance[address] = {
                "source": str(source), "line": text.count("\n", 0, match.start()) + 1,
                "translation_sha256": hashlib.sha256(match.group().encode()).hexdigest(),
            }
    if bodies.keys() != wanted:
        p.error("missing native functions: " + str(wanted - bodies.keys()))
    template = Path(__file__).with_name(a.kind + "_probe.cpp.in").read_text()
    for address, body in bodies.items():
        marker = "// @RETAIL_FUNCTION@" if a.kind == "gain" else "// @RETAIL_FUNCTION_" + address + "@"
        if template.count(marker) != 1:
            raise ValueError("invalid template marker: " + marker)
        template = template.replace(marker, body)
    a.out.mkdir(parents=True, exist_ok=True)
    cpp, exe = a.out/(a.kind + "_probe.cpp"), a.out/(a.kind + "_probe.exe")
    vectors = a.out/(a.kind + ("-vectors.bin" if a.kind == "mixmap_full" else "-vectors.txt"))
    cpp.write_text(template)
    command = [a.clang, "-std=c++23", "-O2", "-mfma", "-DNDEBUG", "-D_CRT_SECURE_NO_WARNINGS",
               "-I" + str(a.sdk/"include"), "-I" + str(a.sdk/"thirdparty/simde"), str(cpp), "-o", str(exe)]
    subprocess.run(command, check=True)
    subprocess.run([str(exe), str(a.image), *([str(a.mxb)] if a.kind == "mixmap_full" else []), str(vectors)], check=True)
    (a.out/(a.kind + "-provenance.json")).write_text(json.dumps({
        "image_sha256": digest, "functions": provenance,
        "mxb_sha256": hashlib.sha256(a.mxb.read_bytes()).hexdigest() if a.mxb else None, "compiler_command": command,
        "compiler_version": subprocess.check_output([a.clang, "--version"], text=True),
        "vectors_sha256": hashlib.sha256(vectors.read_bytes()).hexdigest(),
    }, indent=2))
    print(vectors)


if __name__ == "__main__":
    main()
