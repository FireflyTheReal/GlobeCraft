# Municipal coverage by country

Last reviewed 2026-10-22, from geoBoundaries' metadata table and the size of every
simplified file. Each country is checked for the German pattern: accurate municipal
borders that replace the Natural Earth state (and country) lines from 402 km down,
and the single municipalities appearing as the view gets closer.

**How it works for every source.** A municipal border segment that runs along a Natural
Earth state line (within 3 km, roughly parallel) *is* that state border, and one along
a country line is that country border (`boundaries/municipal/reference.rs`). National
sources additionally name each municipality's state. So a country only needs a file of
municipality-sized units; the level per country is in `boundaries/adm.rs`.

**Nothing below was checked on screen yet.** "Unsure" marks countries where the data
itself raises a doubt; any country may still show problems on the Mac.

Legend: *units* is the number of municipal units, *km²* their mean area (Germany: 32),
*MB* the one-off download (cached on disk afterwards).

## 1. National sources (6)

| Country | Source |
| --- | --- |
| BRA Brazil | IBGE municipalities, one state at a time |
| CAN Canada | Statistics Canada census divisions, provinces named in the data |
| DEU Germany | BKG VG250 (federal agency), municipalities, Länder named in the data |
| ESP Spain | IGN municipios, provinces named in the data |
| FRA France | IGN ADMIN EXPRESS communes, départements named in the data. Loads only 0.75° around the view (heavy full-resolution data) |
| USA United States of America | US Census TIGERweb counties, states named in the data |

## 2. geoBoundaries, German pattern possible (120)

| Country | Level | Units | km² | MB | geoBoundaries calls them |
| --- | --- | --- | --- | --- | --- |
| AFG Afghanistan | ADM2 | 398 | 1,613 | 2.9 | wuleswali |
| AGO Angola | ADM3 | 558 | 2,236 | 2.5 | Communes |
| ALB Albania | ADM3 | 373 | 76 | 0.6 |  |
| ARM Armenia | ADM2 | 39 | 730 | 0.2 | Municipal Communities |
| AUT Austria | ADM3 | 2,097 | 40 | 6.0 |  |
| AZE Azerbaijan | ADM2 | 79 | 1,092 | 0.3 |  |
| BDI Burundi | ADM2 | 119 | 211 | 0.3 |  |
| BEL Belgium | ADM4 | 589 | 52 | 1.0 |  |
| BEN Benin | ADM3 | 546 | 211 | 0.5 | Arrondissements |
| BFA Burkina Faso | ADM3 | 351 | 779 | 1.5 |  |
| BGD Bangladesh | ADM4 | 5,160 | 27 | 12.5 | Union Councils / Municipal Corporations / City Corporations |
| BGR Bulgaria | ADM2 | 265 | 419 | 0.8 | Municipality |
| BIH Bosnia and Herzegovina | ADM3 | 142 | 362 | 0.6 |  |
| BLR Belarus | ADM2 | 118 | 1,762 | 1.1 | Raion |
| BLZ Belize | ADM2 | 31 | 706 | 0.2 | Constituencies |
| BRN Brunei Darussalam | ADM2 | 38 | 156 | 0.1 | Mukim |
| BTN Bhutan | ADM2 | 205 | 189 | 0.7 | Gewog |
| CHE Switzerland | ADM3 | 2,286 | 18 | 3.7 | Municipality |
| CHL Chile | ADM3 | 345 | 2,188 | 10.9 | Communes |
| CIV Côte d'Ivoire | ADM3 | 510 | 631 | 2.3 | Departments |
| CMR Cameroon | ADM3 | 360 | 1,295 | 1.5 |  |
| COL Colombia | ADM2 | 1,122 | 1,014 | 10.6 |  |
| COM Comoros | ADM3 | 55 | 30 | 0.1 | Commune |
| CPV Cabo Verde | ADM2 | 32 | 127 | 0.2 | freguesia |
| CRI Costa Rica | ADM3 | 472 | 105 | 1.1 |  |
| CUB Cuba | ADM2 | 168 | 655 | 0.9 |  |
| CYP Cyprus | ADM2 | 612 | 15 | 1.2 |  |
| CZE Czechia | ADM3 | 6,257 | 13 | 8.1 |  |
| DJI Djibouti | ADM2 | 11 | 1,971 | 0.0 | Districts |
| DNK Denmark | ADM2 | 98 | 445 | 0.4 | kommune |
| DOM Dominican Republic | ADM2 | 155 | 310 | 0.7 | Municipalities |
| DZA Algeria | ADM3 | 1,540 | 1,500 | 3.5 | Communes |
| ECU Ecuador | ADM2 | 224 | 1,142 | 3.1 | Cantons |
| EGY Egypt | ADM2 | 365 | 2,725 | 1.4 | marakiz and aqsam |
| ERI Eritrea | ADM2 | 58 | 2,087 | 0.6 | districts |
| ETH Ethiopia | ADM3 | 690 | 1,632 | 2.6 |  |
| FIN Finland | ADM3 | 313 | 1,077 | 1.8 |  |
| FJI Fiji | ADM3 | 86 | 243 | 0.6 | Tikina |
| FSM Micronesia (Fed. States of) | ADM2 | 75 | 9 | 0.1 | Municipality |
| GEO Georgia | ADM2 | 68 | 1,031 | 1.3 | Municipality |
| GHA Ghana | ADM2 | 260 | 921 | 3.0 | Districts |
| GIN Guinea | ADM3 | 340 | 720 | 0.6 | sub-prefecture |
| GMB Gambia | ADM3 | 120 | 88 | 0.2 | Ward |
| GNB Guinea-Bissau | ADM2 | 39 | 904 | 0.3 |  |
| GNQ Equatorial Guinea | ADM2 | 31 | 868 | 0.1 |  |
| GRC Greece | ADM3 | 326 | 403 | 1.3 | municipality |
| GTM Guatemala | ADM2 | 342 | 316 | 1.2 | Municipio |
| HND Honduras | ADM2 | 299 | 374 | 1.3 | Municipalities |
| HRV Croatia | ADM2 | 560 | 101 | 2.2 | Municipalities and Towns |
| HTI Haiti | ADM3 | 140 | 193 | 0.7 | Communes |
| HUN Hungary | ADM3 | 3,357 | 28 | 3.5 |  |
| IRL Ireland | ADM2 | 166 | 423 | 1.8 | Local Electoral Areas |
| IRN Iran (Islamic Republic of) | ADM4 | 2,815 | 602 | 4.4 | Dehestan |
| ISL Iceland | ADM2 | 74 | 1,388 | 0.9 | Municipalities |
| ISR Israel | ADM2 | 15 | 1,474 | 0.1 | Subdistrict |
| ITA Italy | ADM4 | 7,901 | 38 | 18.4 |  |
| JOR Jordan | ADM2 | 52 | 1,714 | 0.5 | Nahias |
| JPN Japan | ADM2 | 1,745 | 214 | 6.4 | Subprefectures |
| KHM Cambodia | ADM3 | 1,633 | 110 | 3.5 |  |
| KIR Kiribati | ADM2 | 24 | 37 | 0.1 |  |
| KWT Kuwait | ADM2 | 138 | 118 | 0.2 | Areas |
| LAO Lao People's Democratic Republic | ADM2 | 148 | 1,557 | 1.3 | Districts |
| LBN Lebanon | ADM2 | 26 | 394 | 0.1 | Districts (Aqdya) |
| LBR Liberia | ADM2 | 136 | 705 | 0.7 | Districts |
| LKA Sri Lanka | ADM3 | 330 | 200 | 1.7 | Divisional Secretariat |
| LSO Lesotho | ADM2 | 78 | 392 | 0.7 | constituencies |
| LUX Luxembourg | ADM2 | 105 | 25 | 0.2 |  |
| LVA Latvia | ADM2 | 589 | 110 | 2.4 | pilsētās un pagastos |
| MEX Mexico | ADM2 | 2,457 | 800 | 17.2 | municipios |
| MKD North Macedonia | ADM2 | 84 | 297 | 0.4 | Opštini |
| MLI Mali | ADM3 | 701 | 1,789 | 1.6 | Commune |
| MMR Myanmar | ADM3 | 330 | 2,026 | 4.2 | Township |
| MOZ Mozambique | ADM3 | 411 | 1,915 | 3.8 | Administrative Postos Administrativos |
| MWI Malawi | ADM3 | 245 | 456 | 1.0 | Traditional Authorities |
| MYS Malaysia | ADM3 | 1,859 | 138 | 5.2 | Mukim |
| NGA Nigeria | ADM2 | 774 | 1,179 | 3.7 | Local Government Areas |
| NIC Nicaragua | ADM2 | 153 | 840 | 1.0 |  |
| NLD Netherlands | ADM2 | 344 | 121 | 0.9 | Municipality |
| NOR Norway | ADM2 | 431 | 750 | 4.7 |  |
| NPL Nepal | ADM3 | 774 | 191 | 3.7 | Nagarpalika and Gāunpālikā |
| NZL New Zealand | ADM3 | 245 | 1,093 | 3.2 |  |
| PAK Pakistan | ADM3 | 554 | 1,771 | 2.2 | Tehsil |
| PAN Panama | ADM3 | 633 | 117 | 6.4 |  |
| PHL Philippines | ADM3 | 1,647 | 178 | 7.1 | Municipalities |
| PLW Palau | ADM2 | 77 | 6 | 0.1 | Hamlets |
| PNG Papua New Guinea | ADM3 | 326 | 1,417 | 2.2 | Local Government Areas |
| POL Poland | ADM3 | 2,480 | 126 | 7.3 | Gminas |
| PRK Dem. People's Republic of Korea | ADM2 | 179 | 685 | 0.7 | county, city, special city |
| PRY Paraguay | ADM2 | 247 | 1,618 | 2.7 |  |
| PSE State of Palestine | ADM2 | 16 | 375 | 0.2 | governorate |
| QAT Qatar | ADM2 | 79 | 146 | 0.1 |  |
| ROU Romania | ADM2 | 3,235 | 74 | 10.2 | municipalities |
| RWA Rwanda | ADM3 | 416 | 58 | 1.1 |  |
| SEN Senegal | ADM3 | 121 | 1,626 | 0.6 | arrondissement |
| SGP Singapore | ADM2 | 55 | 14 | 0.1 | Divisions |
| SLB Solomon Islands | ADM4 | 1,344 | 21 | 1.7 | Wards |
| SLE Sierra Leone | ADM3 | 165 | 440 | 1.0 | Chiefdoms |
| SLV El Salvador | ADM2 | 272 | 75 | 0.5 | Municipalities |
| SRB Serbia | ADM2 | 145 | 535 | 0.7 |  |
| STP Sao Tome and Principe | ADM2 | 7 | 141 | 0.0 | Districts |
| SUR Suriname | ADM2 | 62 | 2,320 | 0.2 | Ressorten |
| SVK Slovakia | ADM3 | 2,941 | 17 | 3.2 |  |
| SVN Slovenia | ADM2 | 213 | 96 | 0.6 |  |
| SWZ Eswatini | ADM2 | 53 | 328 | 0.1 |  |
| SYR Syrian Arab Republic | ADM3 | 272 | 692 | 0.8 | sub-district |
| TGO Togo | ADM2 | 37 | 1,534 | 0.2 | Prefectures |
| THA Thailand | ADM2 | 928 | 555 | 8.5 | districts |
| TJK Tajikistan | ADM2 | 58 | 2,435 | 0.3 |  |
| TLS Timor-Leste | ADM2 | 65 | 229 | 0.3 | Administrative Posts |
| TON Tonga | ADM2 | 23 | 23 | 0.1 | district |
| TUN Tunisia | ADM3 | 2,083 | 74 | 2.3 |  |
| TUR Turkey | ADM2 | 999 | 781 | 3.9 | Districts |
| TWN Taiwan | ADM2 | 369 | 100 | 0.9 | Townships |
| UGA Uganda | ADM4 | 1,521 | 159 | 4.4 | Sub-counties |
| URY Uruguay | ADM2 | 124 | 510 | 0.7 | Municipalities |
| UZB Uzbekistan | ADM2 | 199 | 2,266 | 0.7 | Tuman |
| VNM Viet Nam | ADM2 | 708 | 473 | 5.3 | District |
| VUT Vanuatu | ADM3 | 621 | 20 | 0.8 |  |
| WSM Samoa | ADM2 | 43 | 66 | 0.1 | Districts |
| YEM Yemen | ADM2 | 335 | 1,352 | 1.0 | Districts |

## 3. geoBoundaries, unsure (44)

These load and hand over like the others, but the units are coarser than municipalities, the geometry is coarse or outdated, or the download is heavy.

| Country | Level | Units | km² | MB | geoBoundaries calls them | Why unsure |
| --- | --- | --- | --- | --- | --- | --- |
| ARG Argentina | ADM2 | 526 | 5,290 | 5.5 | departments | Districts rather than municipalities (mean area above 3,000 km²). |
| AUS Australia | ADM2 | 547 | 14,055 | 11.1 | Local Government Areas | Local government areas; 11 MB. |
| BOL Bolivia (Plurinational State of) | ADM3 | 339 | 3,153 | 4.3 |  | Districts rather than municipalities (mean area above 3,000 km²). |
| BWA Botswana | ADM2 | 25 | 23,123 | 0.1 |  | Districts rather than municipalities (mean area above 3,000 km²). |
| CAF Central African Republic | ADM3 | 175 | 3,545 | 0.8 | Municipalities | Districts rather than municipalities (mean area above 3,000 km²). |
| CHN China | ADM3 | 2,867 | 3,301 | 34.6 | County, City | County level (ADM3), 35 MB download; ADM2 has the same units with very coarse geometry. |
| COD Democratic Republic of the Congo | ADM2 | 189 | 12,330 | 3.3 | territory, city | Districts rather than municipalities (mean area above 3,000 km²). |
| COG Congo | ADM2 | 46 | 7,428 | 0.3 |  | Districts rather than municipalities (mean area above 3,000 km²). |
| EST Estonia | ADM2 | 215 | 202 | 1.0 |  | 215 municipalities from before the 2017 merger (79 today): outdated. |
| GAB Gabon | ADM2 | 49 | 5,355 | 0.3 | Department | Districts rather than municipalities (mean area above 3,000 km²). |
| GBR United Kingdom | ADM2 | 216 | 1,133 | 0.9 | Counties and Unitary Authorities | Counties and unitary authorities, not municipalities, with very coarse geometry (about 90 points per unit): may look worse than the Natural Earth lines. |
| GUY Guyana | ADM2 | 27 | 7,794 | 0.1 | Neighbourhood Councils | Districts rather than municipalities (mean area above 3,000 km²). |
| IDN Indonesia | ADM2 | 519 | 3,644 | 11.0 | regency, city | Regencies only: geoBoundaries has no lower level. |
| IND India | ADM2 | 736 | 4,450 | 8.0 | District | Districts only: the sub-district file (tehsils) is 40 MB. |
| IRQ Iraq | ADM2 | 101 | 4,320 | 0.5 | Districts | Districts rather than municipalities (mean area above 3,000 km²). |
| JAM Jamaica | ADM2 | 827 | 13 | 1.7 | community | Communities, finer than municipalities (parishes are the states). |
| KAZ Kazakhstan | ADM2 | 174 | 15,693 | 0.9 | District | Districts rather than municipalities (mean area above 3,000 km²). |
| KEN Kenya | ADM3 | 1,452 | 405 | 3.1 | Ward | Wards. |
| KGZ Kyrgyzstan | ADM2 | 41 | 4,699 | 0.4 | Raions | Districts rather than municipalities (mean area above 3,000 km²). |
| KOR Republic of Korea | ADM2 | 229 | 435 | 0.9 | Si, Kun (districts and city districts) | Si/gun/gu with coarse geometry (about 110 points per unit). |
| LTU Lithuania | ADM3 | 544 | 119 | 2.2 |  | Elderships (seniūnijos), finer than the 60 municipalities. |
| MAR Morocco | ADM2 | 75 | 9,104 | 0.6 |  | Districts rather than municipalities (mean area above 3,000 km²). |
| MDG Madagascar | ADM3 | 1,579 | 375 | 7.1 | commune | Communes (ADM3); fokontany (ADM4) are villages. |
| MNG Mongolia | ADM2 | 339 | 4,616 | 1.8 | soum, düüregs | Districts rather than municipalities (mean area above 3,000 km²). |
| MRT Mauritania | ADM2 | 57 | 18,160 | 0.1 | Mauritania | Districts rather than municipalities (mean area above 3,000 km²). |
| NAM Namibia | ADM2 | 109 | 7,562 | 0.5 |  | Districts rather than municipalities (mean area above 3,000 km²). |
| NER Niger | ADM3 | 266 | 4,438 | 0.7 | Communes | Districts rather than municipalities (mean area above 3,000 km²). |
| OMN Oman | ADM2 | 61 | 5,056 | 0.3 | Wilayaf | Districts rather than municipalities (mean area above 3,000 km²). |
| PER Peru | ADM2 | 196 | 6,565 | 4.3 | Provinces | Districts rather than municipalities (mean area above 3,000 km²). |
| PRT Portugal | ADM3 | 2,905 | 32 | 5.7 | Freguesias | Freguesias (civil parishes), not concelhos: the concelho file has only 25 points per unit. |
| RUS Russian Federation | ADM2 | 2,328 | 7,262 | 26.3 | Raion | Raions, 26 MB download. |
| SAU Saudi Arabia | ADM2 | 147 | 13,082 | 1.5 | Governorate | Districts rather than municipalities (mean area above 3,000 km²). |
| SDN Sudan | ADM2 | 189 | 9,902 | 0.8 | District | Districts rather than municipalities (mean area above 3,000 km²). |
| SOM Somalia | ADM2 | 118 | 5,406 | 0.2 | Districts | Districts rather than municipalities (mean area above 3,000 km²). |
| SSD South Sudan | ADM2 | 78 | 8,109 | 0.3 | counties | Districts rather than municipalities (mean area above 3,000 km²). |
| SWE Sweden | ADM3 | 2,652 | 175 | 7.3 |  | Parish-level units (ADM3); the 290 kommuner (ADM2) have very coarse geometry. |
| TCD Chad | ADM2 | 70 | 18,069 | 0.2 | Departments | Districts rather than municipalities (mean area above 3,000 km²). |
| TKM Turkmenistan | ADM2 | 66 | 7,477 | 0.3 |  | Districts rather than municipalities (mean area above 3,000 km²). |
| TZA United Republic of Tanzania | ADM3 | 3,644 | 243 | 9.2 |  | Wards. |
| UKR Ukraine | ADM3 | 10,375 | 58 | 19.6 | Village Councils | Village councils from before the 2020 hromada reform: outdated. |
| VEN Venezuela (Bolivarian Republic of) | ADM2 | 335 | 3,156 | 3.2 | Municipios | Districts rather than municipalities (mean area above 3,000 km²). |
| ZAF South Africa | ADM3 | 213 | 5,732 | 2.7 | Local municipality | Districts rather than municipalities (mean area above 3,000 km²). |
| ZMB Zambia | ADM2 | 116 | 6,475 | 3.6 | Districts | Districts rather than municipalities (mean area above 3,000 km²). |
| ZWE Zimbabwe | ADM3 | 1,961 | 199 | 5.2 | Ward | Wards. |

## 4. Only one level of subdivision (27)

The country has no states between it and its municipalities (or geoBoundaries has only that level). The municipalities load from ADM1, and Natural Earth already draws the same lines as "states", so they hand over like state borders; there is no separate state layer above them.

| Country | Level | Units | km² | MB | geoBoundaries calls them |
| --- | --- | --- | --- | --- | --- |
| AND Andorra | ADM1 | 7 | 67 | 0.0 | Parish |
| ARE United Arab Emirates | ADM1 | 7 | 10,145 | 0.2 |  |
| ATG Antigua and Barbuda | ADM1 | 8 | 53 | 0.0 | Parish and Dependency |
| BHR Bahrain | ADM1 | 4 | 190 | 0.0 |  |
| BHS Bahamas | ADM1 | 32 | 418 | 0.9 | Districts |
| BRB Barbados | ADM1 | 11 | 39 | 0.0 | Parish |
| DMA Dominica | ADM1 | 10 | 75 | 0.0 | Parish |
| GRD Grenada | ADM1 | 7 | 50 | 0.0 | Parish |
| GRL Greenland | ADM1 | 6 | 352,922 | 1.6 | Municipalities |
| KNA Saint Kitts and Nevis | ADM1 | 14 | 19 | 0.0 | parish |
| LBY Libya | ADM1 | 22 | 73,807 | 0.1 | Districts |
| LCA Saint Lucia | ADM1 | 10 | 61 | 0.0 | District |
| LIE Liechtenstein | ADM1 | 11 | 15 | 0.0 |  |
| MDA Republic of Moldova | ADM1 | 37 | 915 | 0.3 | Districts |
| MDV Maldives | ADM1 | 21 | 1,438 | 0.0 |  |
| MHL Marshall Islands | ADM1 | 24 | 272 | 0.0 |  |
| MLT Malta | ADM1 | 68 | 5 | 0.1 | Local Council |
| MNE Montenegro | ADM1 | 23 | 604 | 0.1 |  |
| MUS Mauritius | ADM1 | 12 | 166 | 0.1 | Districts and Outer Islands of Mauritius |
| NIU Niue | ADM1 | 14 | 19 | 0.0 | village |
| NRU Nauru | ADM1 | 14 | 2 | 0.0 | District |
| SMR San Marino | ADM1 | 9 | 7 | 0.0 |  |
| SYC Seychelles | ADM1 | 26 | 18 | 0.1 |  |
| TTO Trinidad and Tobago | ADM1 | 14 | 367 | 0.0 |  |
| TUV Tuvalu | ADM1 | 8 | 3 | 0.0 |  |
| VCT Saint Vincent and the Grenadines | ADM1 | 6 | 65 | 0.0 | Parishes |
| XKX Kosovo | ADM1 | 48 | 479 | 0.1 | Municipalities |

## 5. Not possible with geoBoundaries (35)

Territories with no subdivision in geoBoundaries. They fall back to OpenStreetMap cells (Overpass), which is untested for them: municipal borders may or may not appear.

| Code | Name |
| --- | --- |
| ABW | Aruba |
| AIA | Anguilla |
| ASM | American Samoa |
| ATA | Antartica |
| BES | Bonaire Sint Eustatius and Saba |
| BLM | Saint Barthélemy |
| BMU | Bermuda |
| COK | Cook Islands |
| CUW | Curaçao |
| CYM | Cayman Islands |
| FLK | Falkland Islands (Malvinas) |
| FRO | Faroe Islands |
| GGY | Guernsey |
| GIB | Gibraltar |
| GLP | Guadeloupe |
| GUF | French Guiana |
| GUM | Guam |
| IMN | Isle of Man |
| MCO | Monaco |
| MNP | Northern Mariana Islands |
| MSR | Montserrat |
| MTQ | Martinique |
| MYT | Mayotte |
| NCL | New Caledonia |
| PCN | Pitcairn Islands |
| PRI | Puerto Rico |
| PYF | French Polynesia |
| REU | Réunion |
| SHN | Saint Helena |
| TCA | Turks and Caicos Islands |
| TKL | Tokelau |
| VAT | Holy See |
| VGB | British Virgin Islands |
| VIR | United States Virgin Islands |
| WLF | Wallis and Futuna Islands |

## Natural Earth codes

GlobeCraft finds the country under the view from Natural Earth, whose codes differ from
geoBoundaries' for a few countries: `PSX` → `PSE`, `SDS` → `SSD`, `KOS` → `XKX`,
`SAH` → `ESH` (Western Sahara has no geoBoundaries file and falls back to OSM). Natural
Earth units without a geoBoundaries country (Somaliland, Northern Cyprus, sovereign base
areas, disputed areas) also fall back to OSM.

## How the levels were chosen

For each country: the deepest geoBoundaries level that refines the level above it by at
least 20 % more units, whose units average at least 3 km² (so not villages or parcels),
with at most 40,000 units and a simplified file under 40 MB. Then by hand: Austria,
Czechia and Slovakia use their municipalities instead of cadastral communities; Sri
Lanka, Madagascar, Fiji, Rwanda, Burundi and Sierra Leone the level above villages or
enumeration areas; South Africa local municipalities instead of wards; Estonia ADM2;
South Korea si/gun/gu; China ADM3 for its geometry. Re-run the review when geoBoundaries
publishes a new release.

