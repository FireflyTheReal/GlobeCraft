import json,sys,re,os,subprocess
G=sys.argv[1]
s=open(os.path.join(os.path.dirname(__file__), '..', '..', 'crates/globecraft-data/src/boundaries/adm.rs')).read()
lv={}
for l in ('ADM1','ADM2','ADM3','ADM4'):
    for c in re.search(l+r'_COUNTRIES: &str = "(.*?)";',s,re.S).group(1).replace('\\','').split(): lv[c]=l
st={}
for l in ('ADM2','ADM3'):
    for c in re.search(l+r'_STATES: &str = "(.*?)";',s).group(1).split(): st[c]=l
alias={"PSX":"PSE","SDS":"SSD","KOS":"XKX","SAH":"ESH"}
pairs=json.load(open(G+'/audit/pairs.json'))
countries=sorted({p['a'] for p in pairs}|{p['b'] for p in pairs})
for ne in countries:
    gb=alias.get(ne,ne); l=lv[gb]; levels=[l]
    sl=st.get(gb,'ADM1')
    if sl!=l: levels.append(sl)
    for lvl in levels:
        f=f'{G}/{ne}-{lvl}.geojson'
        if os.path.exists(f) and os.path.getsize(f)>0: continue
        url=f'https://media.githubusercontent.com/media/wmgeolab/geoBoundaries/main/releaseData/gbOpen/{gb}/{lvl}/geoBoundaries-{gb}-{lvl}_simplified.geojson'
        r=subprocess.run(['curl','-sSfL','--retry','3','-o',f,url])
        print(ne,lvl,'ok' if r.returncode==0 else 'FAILED',flush=True)
print('done')
