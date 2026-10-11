"""Independent reader qualification for generated synthetic fixtures.
Requires PyMuPDF and psd-tools in an isolated development environment.
Not a claim of Photoshop or Acrobat execution.
"""
from pathlib import Path
import json, sys, io
import pymupdf as fitz
import numpy as np
from PIL import Image
from psd_tools import PSDImage

root=Path(sys.argv[1]); report={"mupdf":fitz.VersionBind,"photoshop":"NOT RUN: application unavailable","acrobat":"NOT RUN", "pdf":[],"psd":[]}
def expected(path):
    im=Image.open(path).convert("RGBA")
    white=Image.new("RGBA",im.size,"white");white.alpha_composite(im)
    return np.asarray(white.convert("RGB"),dtype=np.int16)
def compare(d,path):
    # Reopen after visibility configuration changes; no cached display list is reused.
    fresh=fitz.open(stream=d.tobytes(),filetype="pdf")
    pix=fresh[0].get_pixmap(alpha=False)
    pix.save(root/("mupdf-"+path.name))
    got=np.frombuffer(pix.samples,dtype=np.uint8).reshape(pix.height,pix.width,3).astype(np.int16)
    want=expected(path)
    assert got.shape==want.shape
    diff=np.abs(got-want)
    result={"maxChannelError":int(diff.max()),"meanChannelError":float(diff.mean())}
    assert result["maxChannelError"]<=8, result
    return result
for depth in [8,16,32]:
    d=fitz.open(root/f"fixture-{depth}.pdf")
    ocgs=d.get_ocgs()
    assert len(d)==1 and len(ocgs)==6
    assert {v["name"] for v in ocgs.values()}=={"Paper","Color noise","Paint group","Multiply","Screen","Hidden \u84dd"}
    default_off=set(d.get_layer().get("off",[]))
    assert len(default_off)==1
    for ref, group in ocgs.items(): group["on"] = ref not in default_off
    assert sum(not item["on"] for item in d.layer_ui_configs() if item["type"]=="checkbox")==1
    # Resolve every indirect reference through the independent parser.
    for xref in range(1,d.xref_length()): assert d.xref_object(xref)
    record={"depth":depth,"pageCount":len(d),"ocgCount":len(ocgs),"composite":compare(d,root/f"fixture-{depth}.png")}
    if depth==8:
        record["toggles"]=[]
        for item in json.loads((root/"toggles.json").read_text()):
            on=[k for k,v in ocgs.items() if v["on"] != (v["name"]==item["name"])]
            off=[k for k in ocgs if k not in on]
            d.set_layer(-1,basestate="ON",on=on,off=off)
            result=compare(d,root/item["expected"])
            record["toggles"].append({"name":item["name"],**result})
    report["pdf"].append(record)
    for ext in ["psd","psb"]:
        psd=PSDImage.open(root/f"fixture-{depth}.{ext}")
        def tree(layers):
            return [{"name":l.name,"visible":l.visible,"opacity":l.opacity,"blend":str(l.blend_mode),"mask":l.has_mask(),"children":tree(l) if l.is_group() else []} for l in layers]
        structure=tree(psd)
        assert [l["name"] for l in structure]==["Paper","Color noise","Paint group","Hidden \u84dd"],structure
        assert [l["name"] for l in structure[2]["children"]]==["Multiply","Screen"]
        assert not structure[3]["visible"] and structure[2]["children"][0]["mask"]
        leaves=[l for l in psd.descendants() if not l.is_group()]
        assert len(leaves)==5
        for l in leaves:
            image=l.topil()
            assert image is not None and image.width>0 and image.height>0
        assert psd.depth==depth and psd.size==(64,48)
        composite=psd.composite(ignore_preview=True).convert("RGBA")
        delta=np.abs(np.asarray(composite,dtype=np.int16)-np.asarray(Image.open(root/f"fixture-{depth}.png").convert("RGBA"),dtype=np.int16))
        assert delta.max()<=2, (depth,ext,int(delta.max()))
        composite.save(root/f"independent-{depth}-{ext}.png")
        report["psd"].append({"depth":depth,"container":ext,"leafCount":len(leaves),"tree":structure,"independentCompositeMaxChannelError":int(delta.max())})
report["complex"] = []
for ext in ["psd", "psb"]:
    psd=PSDImage.open(root/f"complex.{ext}")
    layers={l.name:l for l in psd.descendants()}
    assert len([l for l in layers.values() if not l.is_group()])==8
    assert layers["Editable title"].kind=="type"
    assert layers["Editable title"].text.startswith("Editable title")
    assert layers["Editable vector"].has_vector_mask()
    assert any(type(effect).__name__=="DropShadow" and effect.enabled for effect in layers["Editable vector"].effects)
    assert layers["Invert adjustment"].kind=="invert"
    assert layers["Nested editing group"].parent.name=="Paint group"
    report["complex"].append({"container":ext,"layers": {name:l.kind for name,l in layers.items()}})
(root/"independent-qualification.json").write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding="utf-8")
print(json.dumps({"pdfFiles":len(report["pdf"]),"psdPsbFiles":len(report["psd"]),"complexPsdPsbFiles":len(report["complex"]),"status":"PASS","photoshop":report["photoshop"]}))
