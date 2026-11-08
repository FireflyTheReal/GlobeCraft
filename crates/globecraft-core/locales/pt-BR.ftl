# Embedded UI catalog. Keep keys and variables in sync across locales.

kml-open-dialog-title = Abrir KML ou KMZ

kml-save-dialog-title = Salvar KML ou KMZ como

screenshot-empty-error = Nada para salvar ainda.

kml-drop-type-error = Só é possível soltar arquivos KML, KMZ, GeoJSON, GPX ou CSV aqui.

globe-placemark-action = Adicionar local aqui

globe-fly-to-action = Voar até aqui

globe-ruler-point-action = Adicionar ponto na régua

globe-copy-coordinates-action = Copiar coordenadas

globe-context-menu-hint = Clique sobre o globo.

app-ready-status = Pronto. Arraste para girar, role para aproximar.

search-empty-status = Nenhum resultado encontrado.

search-pending-status = Buscando…

search-input-placeholder = Buscar lugar, endereço ou “lat, lon”

search-submit-action = Buscar

settings-open-tooltip = Configurações
settings-language-label = Idioma
settings-language-system = Idioma do sistema ({ $language })
settings-language-system-fallback = Idioma do sistema ({ $language } — idioma alternativo)

navigation-compass-tooltip = Clique para alinhar ao norte

navigation-north-tooltip = Norte para cima

navigation-reverse-tooltip = Girar a vista 180°

navigation-rotate-left-tooltip = Girar para a esquerda (segure)

navigation-rotate-right-tooltip = Girar para a direita (segure)

navigation-pan-north-tooltip = Mover para o norte (segure)

navigation-pan-south-tooltip = Mover para o sul (segure)

navigation-pan-west-tooltip = Mover para oeste (segure)

navigation-pan-east-tooltip = Mover para leste (segure)

navigation-zoom-in-tooltip = Aproximar (segure)

navigation-zoom-out-tooltip = Afastar (segure)

navigation-zoom-slider-tooltip = Arraste para ajustar o zoom
navigation-east-tooltip = Leste para cima
navigation-south-tooltip = Sul para cima
navigation-west-tooltip = Oeste para cima
navigation-ring-tooltip = Arraste o anel para girar a vista
navigation-tilt-tooltip = Inclinação { $degrees }°: arraste para inclinar a vista, clique duas vezes para olhar direto para baixo
navigation-globe-tooltip = Arraste para girar o globo

settings-title = Configurações

settings-text-section-title = Tamanho do texto

settings-ui-scale-label = Botões e painéis

settings-map-scale-label = Texto do mapa (nomes, régua, escala)

settings-folders-section-title = Onde salvar os arquivos

settings-folder-browse-action = Procurar…

settings-screenshot-folder-label = Capturas de tela (PNG):

settings-screenshot-folder-dialog-title = Pasta das capturas

settings-kml-folder-label = Arquivos KML/KMZ:

settings-kml-folder-dialog-title = Pasta dos KML/KMZ

settings-folder-creation-help = As pastas são criadas automaticamente ao salvar.

settings-reset-action = Restaurar padrões

settings-cache-section-title = Cache do mapa
settings-cache-clear-action = Limpar cache em disco
settings-cache-clear-help = Remove os tiles baixados do mapa. Eles serão baixados novamente quando necessário.
settings-cache-clearing-label = Limpando cache…
settings-cache-cleared-status = Cache de tiles em disco limpo.
settings-cache-clear-error = Could not clear on-disk cache: { $error }
settings-cache-disk-usage = { $size } em disco
settings-cache-measuring-label = Calculando…
settings-cache-usage-unavailable = Cache size unavailable

layers-section-title = Camadas

layers-search-placeholder = Buscar camadas…
layers-search-clear = Limpar a busca de camadas
layers-search-active-only = Somente ativas
layers-search-active-help = Mostrar somente as camadas ativadas.
layers-search-empty = Nenhuma camada corresponde a “{ $query }”.
layers-search-empty-active = Nenhuma camada está ativada.

wayback-loading-status = Carregando datas disponíveis…

wayback-older-tooltip = Mais antiga

wayback-newer-tooltip = Mais recente

wayback-fallback-help = Nem todo lugar tem imagem em todas as datas; áreas sem imagem mostram a versão mais próxima disponível.

layers-grid-toggle = Grade de latitude/longitude

layers-country-borders-toggle = Limites dos países (mundo todo)

layers-state-borders-toggle = Estados / províncias (mundo todo)
layers-state-altitude-label = Estados visíveis abaixo

layers-place-labels-toggle = Nomes de países, estados e cidades

layers-municipal-borders-toggle = Limites municipais


layers-boundary-attribution = Fronteiras e nomes: colaboradores do OpenStreetMap (ODbL) via OpenFreeMap e OpenMapTiles; Natural Earth (domínio público) sem internet.

boundaries-loading-status = Carregando limites e nomes…
boundaries-osm-loading-status = Carregando limites do OpenStreetMap…

layers-atmosphere-toggle = Atmosfera e estrelas

layers-sunlight-toggle = Luz do sol em tempo real (dia/noite)
layers-stars-toggle = Estrelas
layers-milky-way-toggle = Via Láctea (fundo da NASA)
layers-night-lights-toggle = Luzes das cidades à noite (NASA Black Marble)

layers-auto-rotation-toggle = Girar automaticamente

layers-flights-toggle = Aviões reais (OpenSky + ADSB.lol)
aircraft-summary-status = Aviões reais rastreados: { $count } | OpenSky: { $global } | ADSB.lol: { $local }
aircraft-loading-status = carregando
aircraft-zoom-in-status = aproxime o mapa
aircraft-updated-seconds-status = há { $seconds } s
aircraft-updated-status = há { $minutes } min
aircraft-source-error = { $source }: { $error }
aircraft-hover-tooltip = { $code }  (ICAO { $hex })
    Altitude: { $altitude }  |  Velocidade: { $speed }
    Rumo: { $heading }  |  Taxa vertical: { $vertical_rate }
    Posição há { $age } s
cmd-ui-legend = Legenda
cmd-ui-legend-help = Mostra o que as cores no globo significam: as altitudes de um voo selecionado, o radar de chuva, a aurora e os pontos de interesse. Abre sozinha quando um deles aparece.
polar-imagery-attribution = Imagens polares: NASA Blue Marble e MODIS (GIBS)
antarctic-claim-united-kingdom = Território Antártico Britânico (Reino Unido)
antarctic-claim-argentina = Antártida Argentina (Argentina)
antarctic-claim-chile = Território Chileno Antártico (Chile)
antarctic-claim-norway = Terra da Rainha Maud (Noruega)
antarctic-claim-australia = Território Antártico Australiano (Austrália)
antarctic-claim-france = Terra Adélia (França)
antarctic-claim-new-zealand = Dependência de Ross (Nova Zelândia)
antarctic-claim-unclaimed = Terra de Marie Byrd (não reivindicada)
legend-title = Legenda
legend-empty = Nada no globo tem uma legenda de cores agora.
legend-flight-altitude = Voo selecionado: altitude
legend-on-ground = no solo
legend-radar = Radar de chuva
legend-radar-band = { $name } ({ $range })
legend-radar-light = fraca
legend-radar-moderate = moderada
legend-radar-heavy = forte
legend-radar-very-heavy = muito forte
legend-radar-extreme = extrema, possível granizo
legend-aurora = Aurora: probabilidade
legend-poi = Pontos de interesse
poi-group-food = Comida e bebida
poi-group-shopping = Compras
poi-group-lodging = Hospedagem
poi-group-culture = Cultura
poi-group-health = Saúde
poi-group-education = Educação
poi-group-transport = Transporte
poi-group-nature = Natureza e parques
poi-group-terrain = Picos
poi-group-water = Água
poi-group-worship = Locais de culto
poi-group-services = Serviços
cmd-flight-select = Selecionar um voo
cmd-flight-select-help = Acompanha um avião: a rota já voada, de onde vem e para onde vai, e a posição a cada poucos segundos.
cmd-flight-clear = Fechar o voo
cmd-flight-clear-help = Deixa de acompanhar o avião selecionado.
cmd-satellite-select = Selecionar um satélite
cmd-satellite-select-help = Mostra uma volta completa da órbita de um satélite na altura real, prevista a partir dos elementos orbitais, e um cartão com a altura e a velocidade.
cmd-satellite-clear = Fechar o satélite
cmd-satellite-clear-help = Esconde a órbita e o cartão do satélite selecionado.
satellite-card-catalogue = NORAD { $norad } • { $group }
satellite-card-height = Altura { $height }
satellite-card-speed = Velocidade { $speed }
satellite-card-over = Sobre { $place }
satellite-card-orbit = Uma órbita em { $period } min • inclinação { $inclination }°
satellite-card-source = { $model ->
        [sgp4] Prevista com SGP4 a partir de elementos da CelesTrak de { $days } dias
       *[simple] Estimada a partir de elementos da CelesTrak de { $days } dias
    }
flight-card-route = { $from } → { $to }
flight-card-route-loading = Buscando a rota…
flight-card-route-unknown = Rota desconhecida
flight-card-route-unsure = A rota publicada pode não ser a deste voo.
flight-card-flown = Voando há { $time }
flight-card-to-go = Faltam { $distance }
flight-card-track-loading = Carregando a rota voada…
flight-card-error = Sem rota voada: { $error }
flight-card-built = fabricado em { $year }
flight-card-altitude = { $feet } pés
flight-card-gps-altitude = GPS { $feet } pés
flight-card-level = nivelado
flight-card-vertical-rate = { $rate } pés/min
flight-card-selected-altitude = piloto automático { $feet } pés
flight-card-ground-speed = { $knots } nós em relação ao solo
flight-card-indicated-airspeed = IAS { $knots } nós
flight-card-true-airspeed = TAS { $knots } nós
flight-card-mach = Mach { $mach }
flight-card-track = rumo { $degrees }°
flight-card-selected-heading = proa do piloto automático { $degrees }°
flight-card-wind = vento de { $degrees }° a { $knots } nós
flight-card-temperature = { $celsius } °C lá fora
flight-card-squawk = squawk { $code }
flight-card-category = categoria { $category }
flight-card-position-source = posição por { $source }
flight-card-emergency = Emergência: { $kind }
flight-card-source = Rota e trajeto: ADSB.lol (ODbL)
layers-weather-toggle = Mapa do clima (radar de chuva ao vivo)
layers-clouds-toggle = Mapa das nuvens por satélite (Matt Eason)
layers-live-overlays-help = Aviões reais, com cobertura incompleta: atualização global a cada ~17 min e local a cada ~90 s. Nuvens ~3 h; radar de chuva ~10 min. Requer internet.

navigation-section-title = Navegação

navigation-reset-north-action = Norte para cima

navigation-global-view-action = Visão global

search-results-label = Resultados da busca:

placemarks-section-title = Locais (Placemarks)

placemarks-name-placeholder = Nome

placemarks-add-center-action = No centro

placemarks-fly-to-action = Ir

kml-drop-zone-hint = Arraste um arquivo .kml ou .kmz aqui

kml-save-action = Salvar

kml-save-tooltip = Salva na pasta de KML/KMZ (veja Configurações)

kml-save-as-action = Salvar como…

kml-open-action = Abrir KML/KMZ…

ruler-section-title = Régua

ruler-enable-toggle = Medir distâncias (clique no globo)

ruler-clear-action = Limpar régua

export-section-title = Exportar

screenshot-save-action = Salvar captura (PNG)

settings-folders-action = Mudar pastas…

controls-help-toggle = Mostrar ajuda de controles

controls-help-text = • Arrastar (esq.): girar o globo
    • Botão direito/Shift+arrastar/giro com dois dedos: girar a vista
    • Roda/pinça ou +/-: zoom
    • Shift+rolagem / Shift+arrastar para cima ou baixo: inclinar   • U: olhar para baixo
    • Duplo clique: voar até o ponto
    • Setas: mover   • N: norte para cima
    • Botão direito (clique): menu de contexto

placemarks-fallback-name = Sem nome

provider-satellite-label = Satélite (Esri World Imagery)
provider-night-lights-label = A Terra à noite (NASA Black Marble 2016)

provider-wayback-label = Imagens históricas (Esri Wayback)

provider-streets-label = Ruas (OpenStreetMap)

provider-topography-label = Relevo/Topografia (OpenTopoMap)

provider-imagery-attribution = Imagens © Esri, Maxar, Earthstar Geographics

provider-streets-attribution = © contribuidores do OpenStreetMap

# Whole messages: arguments retain the original numeric formatting.

kml-save-success = Salvo em { $path }

kml-save-error = Erro ao salvar: { $error }

kml-import-success =
    { $count ->
        [1] { $count } local importado.
       *[other] { $count } locais importados.
    } — { $path }

kml-open-error = Erro ao abrir: { $error }

screenshot-save-success = Captura salva: { $path }

screenshot-save-error = Erro: { $error }

search-navigation-status = Indo para { $destination }

search-results-status =
    { $count ->
        [1] { $count } resultado encontrado.
       *[other] { $count } resultados encontrados.
    }

search-request-error = Erro na busca: { $error }

wayback-snapshot-attribution = { $attribution } — imagem de { $date }

placemarks-generated-name = Local { $number }

wayback-availability-status = { $count } versões disponíveis

export-folder-label = Pasta: { $path }

ruler-measurement-summary = Pontos: { $count }   Total: { $distance }

statusbar-tiles-loading = carregando { $count } tiles

statusbar-camera-center = Centro: { $latitude }°, { $longitude }°

statusbar-camera-altitude = Altitude da câmera: { $distance }
statusbar-cursor-elevation = Elevação: { $distance }


settings-uncap-fps-toggle = FPS sem limite (requer reiniciar)
settings-gpu-globe-toggle = Desenhar o globo na GPU (beta, requer reinício)
settings-gpu-globe-help = Mais rápido no Apple Silicon. Vistas próximas, a grade e qualquer problema da GPU continuam usando o renderizador da CPU. Desative se o globo parecer errado.
settings-frame-sync-help = Por padrão, a apresentação é sincronizada com a taxa de atualização da tela. G-SYNC/FreeSync é configurado na tela e no driver gráfico.

# Explorer / Visualização e novas camadas — conteúdo bilíngue
top-explore-action = Explorar +
top-earth = Terra
top-moon = Lua
top-custom-planet = Planeta customizável
explorer-title = Explorar · locais, céu e planetas
explorer-selected-location = Local selecionado
explorer-search-place = Pesquise um local para ver informações
explorer-latlon = Latitude { $latitude } · Longitude { $longitude }
explorer-center = Centro: { $latitude }, { $longitude }
explorer-place-photos = Fotos do local
explorer-photos-opened = Galeria de fotos aberta no navegador
explorer-view-osm = Detalhes no OpenStreetMap
explorer-map-opened = Mapa aberto no navegador
explorer-photo-copyright = As fotos são pesquisadas no Wikimedia Commons e seus direitos pertencem aos autores. Os detalhes são exibidos no navegador, não são avaliações do Google Maps.
explorer-narrated-tour = Passeio narrado (TTS)
explorer-tour-default = Estamos explorando { $place }. Explore o local no globo e escolha pontos de interesse para continuar seu passeio.
explorer-read-aloud = ▶ Narrar texto
explorer-speech-sent = Texto enviado ao sintetizador de voz
explorer-globe-layers = Camadas no globo
explorer-layer-info = As camadas georreferenciadas são exibidas diretamente no globo com dados on-line (requer internet).
explorer-cloud-credit = Imagens de nuvens: Matt Eason / Live Cloud Maps
explorer-cloud-source = Live Cloud Maps — autor e código-fonte
explorer-cloud-license = Contém dados modificados da EUMETSAT · CC0 1.0
explorer-radar-credit = Fonte do radar: RainViewer
explorer-radar-source = Radar meteorológico RainViewer
explorer-radar-explanation = Verde/amarelo/vermelho indicam a intensidade aproximada da chuva (refletividade do radar), não a temperatura infravermelha do satélite. A cobertura e atualização variam.
explorer-bodies = Corpos celestes
explorer-earth = Terra
explorer-moon = Lua
explorer-custom-planet = Planeta customizável
explorer-moon-fixed = A Lua é fixa e não editável. As imagens abaixo pertencem exclusivamente ao planeta customizável.
explorer-name = Nome:
explorer-image = Imagem:
explorer-browse = Escolher
explorer-display-planet = Mostrar imagem do planeta
explorer-planet-displayed = Planeta exibido no globo
explorer-save-planet = Salvar planeta…
explorer-load-planet = Carregar planeta…
explorer-planet-loaded = Planeta e céu carregados no globo
explorer-planet-saved = Planeta e céu incorporados em { $path }
explorer-file-working = Processando arquivo…
explorer-embedded-image = Imagem incorporada
explorer-sky-galaxy = Céu e galáxia personalizados
explorer-image-path = Caminho da imagem
explorer-display-galaxy = Exibir galáxia
explorer-sky-applied = Céu aplicado ao fundo do globo
explorer-sky-preview = O céu personalizado é pré-visualizado aqui; não é usado como skybox no renderizador da Terra.
live-clouds-failed = Falha ao atualizar nuvens
live-cached-image =  (exibindo imagem em cache)
live-clouds-ok = Nuvens: Matt Eason (atualização a cada 3 horas)
live-clouds-loading = Nuvens: carregando imagem de satélite…
live-radar-failed = Falha ao atualizar radar
live-radar-age = Radar de chuva: observação de aproximadamente { $minutes } min atrás
live-radar-loading = Radar de chuva: carregando dados do RainViewer…
sidebar-cloud-source = Dados de nuvens: Matt Eason / EUMETSAT
sidebar-radar-source = Dados de radar: RainViewer
live-cloud-credit = Matt Eason · Live Cloud Maps · Contém dados modificados da EUMETSAT
live-radar-credit = Radar de precipitação: RainViewer (rainviewer.com)

# Native desktop menus
menu-file-title = Arquivo
menu-kml-open = Abrir KML/KMZ…
menu-add-data = Adicionar dados…
menu-kml-save = Salvar KML/KMZ
menu-kml-save-as = Salvar KML/KMZ como…
menu-screenshot-save = Salvar captura de tela
menu-quit-action = Sair do GlobeCraft


settings-distance-units-label = Unidades de distância
settings-units-system = Localidade do sistema ({ $unit })
settings-units-metric = Quilômetros / metros
settings-units-imperial = Milhas / pés

ruler-unit-automatic = Automático
ruler-unit-centimeters = Centímetros
ruler-unit-meters = Metros
ruler-unit-kilometers = Quilômetros
ruler-unit-inches = Polegadas
ruler-unit-feet = Pés
ruler-unit-yards = Jardas
ruler-unit-miles = Milhas
ruler-unit-nautical-miles = Milhas náuticas
ruler-unit-smoots = Smoots
ruler-unit-degrees = Graus
ruler-unit-arcseconds = Segundos de arco
ruler-unit-football-fields = Campos de futebol
ruler-unit-american-football-fields = Campos de futebol americano
ruler-units-label = Unidades
ruler-distance-smoots = { $value } smoots
ruler-distance-football-fields = { $value } campos de futebol
ruler-unit-automatic-help = Usa as unidades de distância selecionadas nas Configurações.
ruler-unit-centimeters-help = 1 centímetro = 0,01 metro.
ruler-unit-meters-help = 1 metro = 100 centímetros.
ruler-unit-kilometers-help = 1 quilômetro = 1.000 metros.
ruler-unit-inches-help = 1 polegada = 2,54 centímetros.
ruler-unit-feet-help = 1 pé = 12 polegadas.
ruler-unit-yards-help = 1 jarda = 3 pés.
ruler-unit-miles-help = 1 milha = 1.760 jardas.
ruler-unit-nautical-miles-help = 1 milha náutica = 1.852 metros.
ruler-unit-smoots-help = 1 smoot = 67 polegadas.
ruler-unit-degrees-help = Ângulo central ao longo do trajeto medido no globo.
ruler-unit-arcseconds-help = 1 segundo de arco = 1/3.600 de grau de ângulo central.
ruler-unit-football-fields-help = Um campo de futebol americano equivale a 100 jardas, sem as zonas de pontuação.

altitude-edit-tooltip = Definir a altitude exata da câmera
altitude-edit-title = Altitude da câmera
altitude-apply-action = Aplicar
altitude-cancel-action = Cancelar
altitude-input-error = Digite um número entre { $minimum } e { $maximum } { $unit }.

settings-hide-navigation-controls-toggle = Ocultar controles de navegação
settings-navigator-style-label = Controles de navegação:
settings-navigator-globe = Globo
settings-navigator-classic = Clássico

settings-close-action = Fechar

ruler-point-edit-help = Alt+clique: novo trajeto · Arraste para mover · Cursor + Delete: remover
ruler-point-edit-help-macos = Option+clique: novo trajeto · Arraste para mover · Cursor + Delete: remover
compass-north = N
compass-northeast = NE
compass-east = L
compass-southeast = SE
compass-south = S
compass-southwest = SO
compass-west = O
compass-northwest = NO
compass-heading = { $degrees }° { $direction }
compass-heading-tooltip = Direção no topo do mapa, no sentido horário a partir do norte verdadeiro. Clique para alinhar ao norte.
layers-grid-numbers-toggle = Mostrar números
ruler-bearing-label = Rumo do último segmento: { $bearing }
ruler-bearing-tooltip = Rumo inicial do círculo máximo do penúltimo ao último ponto, no sentido horário a partir do norte verdadeiro.
grid-coordinate-label = { $degrees }° { $direction }

menu-edit-title = Editar
edit-undo-action = Desfazer
edit-redo-action = Refazer

ruler-points-title = Coordenadas da régua
ruler-path-heading = Trajeto { $number }
ruler-point-row = { $number }: { $latitude }°, { $longitude }°
ruler-point-edit-mode = Editar ponto selecionado
ruler-point-add-mode = Adicionar um ponto
ruler-point-latitude = Latitude (°)
ruler-point-longitude = Longitude (°)
ruler-point-new-path-toggle = Iniciar trajeto separado
ruler-point-input-help = Graus decimais. Latitude negativa indica sul; longitude negativa indica oeste.
ruler-point-apply-action = Aplicar
ruler-point-add-action = Adicionar ponto
ruler-point-new-action = Novo ponto
ruler-point-close-action = Fechar

settings-earth-model-label = Modelo da Terra
earth-model-sphere = Esfera perfeita
earth-model-wgs84 = Realista — WGS 84 (G2296)
earth-model-sphere-help = Esfera original de raio médio. Alternativa opcional ao modelo realista da Terra.
earth-model-wgs84-help = Modelo padrão da Terra, com geometria e geodésicas no elipsoide WGS 84. Referencial G2296: EPSG:10606 (2D), EPSG:10605 (3D); sem transformações de época, movimento tectônico, relevo ou alturas geoidais. Usa o renderizador de globo na CPU.

places-root-name = Meus lugares
places-document-name = Documento
places-folder-name = Nova pasta
places-add-folder-action = + Pasta
places-clear-all-action = Limpar tudo
places-delete-action = Excluir
places-visible-help = Mostrar este lugar e seus filhos no globo
places-empty-help = Abra um arquivo KML ou KMZ ou adicione um lugar. Arraste lugares para pastas para organizá-los.
places-description-placeholder = Descrição

# Worldwide layers and the 3D Easter egg (merged)
flat-earth-title = ✦ TERRA PLANA SECRETA
flat-earth-unlocked = Easter egg Terra Plana desbloqueado! Esc para voltar.
flat-earth-disclaimer = Uma brincadeira visual, não um modelo científico.
flat-earth-hint = Arraste: girar · Shift: mover · Roda: zoom · Duplo clique: reiniciar · Esc: sair
flat-earth-return = Voltar ao globo
flat-earth-image-error = Não foi possível carregar a ilustração secreta.
layers-vessels-toggle = Navios AIS do mundo inteiro
layers-aurora-toggle = Aurora (previsão da NOAA)
ribbon-layer-aurora = Aurora
aurora-loading-status = Carregando a previsão de aurora…
aurora-loaded-status = Previsão para { $time } (em { $minutes } min) • maior chance { $peak } % • mostrada só perto do presente
aurora-credit = Aurora: NOAA SWPC
aircraft-credit = Aeronaves: OpenSky Network · ADSB.lol (ODbL)
vessels-credit = Navios: OpenSeaFeed
vessels-fallback-credit = Navios: Fintraffic Digitraffic (CC BY 4.0)
satellites-credit = Satélites: CelesTrak
traffic-credit = Trânsito: © TomTom
aurora-source = Centro de Previsão do Clima Espacial da NOAA (OVATION)
layers-timezones-toggle = Fusos horários
layers-sea-names-toggle = Nomes de mares e do fundo do mar
ribbon-layer-sea-names = Nomes do mar
sea-names-help = Oceanos, mares, golfos e estreitos, zonas econômicas exclusivas e feições submarinas (montes submarinos, fossas, dorsais…), cada um nas altitudes em que cabe na vista.
sea-names-source = Gazetteer do Marine Regions, Flanders Marine Institute (CC BY 4.0)
sea-names-credit = Nomes: MarineRegions.org
ribbon-layer-timezones = Fusos horários
timezones-loading-status = Carregando os fusos horários…
timezones-loaded-status = { $count } fusos horários • a hora local sob o cursor aparece na barra de status
timezones-source = timezone-boundary-builder (OpenStreetMap, ODbL)
timezones-credit = Fusos horários: timezone-boundary-builder, © colaboradores do OpenStreetMap
statusbar-cursor-local-time = Hora local: { $time }
layers-satellites-toggle = Satélites (órbitas CelesTrak, estimadas)
layers-traffic-toggle = Congestionamento real (TomTom; chave opcional)
layers-traffic-key = Chave de API TomTom (não é salva)
layers-roads-toggle = Estradas (OSM)
layers-railways-toggle = Ferrovias, metrôs e bondes (OSM)
layers-ferries-toggle = Rotas de balsa (OSM)
layers-airports-toggle = Aeroportos (OSM)
layers-poi-toggle = Pontos de interesse (OSM)
layers-peaks-toggle = Picos de montanhas (OSM)
layers-waters-toggle = Rios e nomes de águas (OSM)
layers-parks-toggle = Parques e reservas naturais (OSM)
transport-zoom-help = Aproxime para vê-los (abaixo de { $altitude } de altitude)
transport-loading-status = Carregando…
transport-error-status = Dados do mapa indisponíveis: { $error }

roads-zoom-help = Aproxime para exibir estradas (até { $altitude } de altitude)
roads-legend = Cinza = trânsito desconhecido; vermelho = em obras (não é engarrafamento)

vessels-loading-status = Carregando navios AIS mundiais…
vessels-world-status = { $count } navios • OpenSeaFeed mundial • cobertura parcial, observações até 90 min
vessels-custom-status = { $count } navios • GeoJSON AIS configurado • cobertura do fornecedor
vessels-fallback-status = { $count } navios • contingência Fintraffic (só Finlândia); fonte mundial indisponível
vessels-fallback-source = Fintraffic (contingência)
satellites-loading-status = Carregando órbitas…
satellites-loaded-status = { $count } satélites • previsões SGP4 a partir dos elementos da CelesTrak (não telemetria)
satellites-together = Junto com: { $names }
satellite-group-station = Estações espaciais
satellite-group-navigation = Navegação (GPS, Galileo, GLONASS, BeiDou)
satellite-group-weather = Meteorológicos
satellite-group-bright = Mais brilhantes (visíveis a olho nu)
aircraft-class-unknown = Aeronave (tipo não informado)
aircraft-class-airliner = Avião comercial ou jato executivo
aircraft-class-heavy = Jato pesado
aircraft-class-light = Avião leve
aircraft-class-fast = Jato de alto desempenho
aircraft-class-helicopter = Helicóptero
aircraft-class-glider = Planador
aircraft-class-balloon = Balão ou dirigível
aircraft-class-ultralight = Ultraleve ou asa-delta
aircraft-class-drone = Drone
vessel-class-cargo = Carga
vessel-class-tanker = Petroleiro ou tanque
vessel-class-passenger = Passageiros
vessel-class-fishing = Pesca
vessel-class-high-speed = Embarcação de alta velocidade
vessel-class-tug = Rebocador ou prático
vessel-class-service = Embarcação de serviço
vessel-class-military = Militar ou policial
vessel-class-pleasure = Recreio ou vela
vessel-class-other = Outro
vessel-class-unknown = Não informado
legend-aircraft = Aeronaves
legend-vessels = Navios (maior = mais comprido)
legend-satellites = Satélites
traffic-key-help = Insira uma chave TomTom para tráfego real
traffic-zoom-help = Aproxime o mapa para mostrar o trânsito
traffic-loading-status = Carregando tráfego real…
traffic-loaded-status = Tráfego atualizado há { $minutes } min (verde=livre, amarelo=lento, vermelho=congestionado)

vessels-unnamed = Navio não identificado
vessels-hover-tooltip =
    { $name } • MMSI { $mmsi }
    { $ship_type ->
        [fishing] Navio de pesca
        [tug] Rebocador ou prático
        [military] Navio militar ou de fiscalização
        [pleasure] Embarcação de recreio ou vela
        [passenger] Navio de passageiros
        [cargo] Navio de carga
        [tanker] Petroleiro
        [high_speed] Embarcação de alta velocidade
        [service] Embarcação de serviço
        [other] Outro navio
       *[unknown] Tipo de navio não informado
    } • Destino: { $destination }
    Tamanho: { $dimensions }  |  Calado: { $draught }
    { $latitude }°, { $longitude }° • { $course } • { $speed } kt
    Último sinal AIS há { $minutes } min • cobertura depende da fonte
satellites-hover-tooltip =
    { $name } (NORAD { $norad }) • { $group }
    Período { $period } • Altitude { $altitude } • Inclinação { $inclination }
    { $model ->
        [sgp4] Prevista com SGP4 a partir dos elementos da CelesTrak
       *[simple] Posição orbital estimada • CelesTrak
    }
transport-airport-hover-tooltip =
    { $name }
    IATA { $iata } • ICAO { $icao }
    { $class ->
        [international] Aeroporto internacional
        [public] Aeroporto público
        [regional] Aeroporto regional
        [military] Aeródromo militar
        [private] Aeródromo privado
       *[other] Aeródromo
    }

update-available-label = Atualizar para v{ $version }
update-available-tooltip = Atualização disponível: v{ $version } — clique para baixar e instalar
update-downloading-label = Baixando atualização
update-progress-tooltip = Baixando atualização: { $percent }%
update-installing-label = Instalando atualização
update-installing-tooltip = Verificando e instalando atualização
update-restart-label = Reiniciar para atualizar
update-restart-tooltip = Atualização instalada: v{ $version } — clique para salvar seu trabalho e reiniciar
update-retry-label = Tentar novamente
update-releases-action = Abrir versões
update-saving-label = Salvando antes de reiniciar

sidebar-explore-title = Globo
sidebar-data-title = Dados
data-pane-favorites-title = Favoritos
data-pane-favorites-empty = Marque uma fonte com a estrela para mantê-la aqui.
data-pane-recent-title = Recentes
data-pane-recent-empty = As fontes que você adicionar aparecem aqui.
data-pane-services-title = Biblioteca
data-pane-mine-title = Meus dados
data-pane-star-help = Adicionar aos favoritos
data-pane-unstar-help = Remover dos favoritos
data-pane-open-layers-action = Ordem, opacidade e remoção em Camadas
data-pane-export-help = Exportar favoritos para um arquivo
data-pane-import-help = Importar favoritos de um arquivo
data-pane-export-empty = Marque uma fonte com a estrela primeiro: só os favoritos são exportados.
data-pane-export-success = { $count } { $count ->
        [one] fonte exportada
       *[other] fontes exportadas
    } para { $path }
data-pane-import-success = { $count } { $count ->
        [one] fonte adicionada
       *[other] fontes adicionadas
    } aos seus favoritos a partir de { $path }
sidebar-places-title = Lugares
sidebar-pin-places-action = Fixar
sidebar-pin-places-tooltip = Manter Lugares visível ao alternar painéis
sidebar-unpin-places-tooltip = Desafixar Lugares
sidebar-places-files-title = Arquivos KML / KMZ
sidebar-imagery-title = Imagem de base

sidebar-geography-title = Limites, rótulos e grade
sidebar-transport-title = Transporte
sidebar-weather-title = Meteorologia
sidebar-live-objects-title = Aeronaves, navios e satélites
sidebar-sky-title = Céu e iluminação

menu-application-title = GlobeCraft
menu-view-title = Visualizar
menu-add-title = Adicionar
menu-tools-title = Ferramentas
menu-window-title = Janela
menu-help-title = Ajuda
menu-about-action = Sobre o GlobeCraft
menu-settings-action = Configurações…
menu-sidebar-toggle = Painel lateral
menu-navigation-toggle = Controles de navegação
menu-add-placemark = Marcador no centro da câmera
menu-fullscreen-toggle = Tela cheia
menu-minimize-action = Minimizar
menu-controls-help = Controles e atalhos de teclado
menu-documentation-action = Documentação
menu-report-issue-action = Relatar um problema…
about-version-label = Versão { $version }
about-description = Explore o globo, as camadas ao vivo e seus lugares geográficos.
about-project-link = Projeto e código-fonte

coordinates-format-label = Coordenadas
coordinates-format-dd = Graus decimais (DD)
coordinates-format-dms = Graus, minutos e segundos (DMS)
coordinates-format-ddm = Graus e minutos decimais (DDM)
coordinates-format-utm = UTM / UPS
coordinates-format-mgrs = MGRS
coordinates-precision-degrees = Casas decimais nos graus
coordinates-precision-seconds = Casas decimais nos segundos
coordinates-precision-minutes = Casas decimais nos minutos
coordinates-precision-meters = Casas decimais nos metros
coordinates-precision-grid = Dígitos MGRS por eixo
coordinates-cursor-readout = Cursor: { $coordinates }
coordinates-camera-readout = Centro: { $coordinates }
coordinates-copy-tooltip = { $coordinates }
    Clique para copiar as coordenadas.
coordinates-input-help = A busca aceita DD, DMS, DDM, UTM/UPS e MGRS. Entrada UTM: zona N/S leste norte. A precisão da grade descreve a resolução exibida, não a exatidão da fonte.

settings-category-general = Geral
settings-category-appearance = Aparência
settings-category-navigation = Navegação
settings-category-globe = Globo
settings-category-graphics = Gráficos
settings-category-cache = Dados e cache
settings-category-network = Rede
settings-category-keys = Chaves
settings-keys-intro = Algumas fontes precisam de uma chave gratuita do provedor. As chaves ficam salvas neste computador, separadas das configurações, legíveis só por você, e são enviadas apenas ao próprio serviço. Uma chave definida no ambiente (GLOBECRAFT_TOMTOM_KEY etc.) tem prioridade.
settings-keys-get-one = Obter uma chave gratuita
settings-keys-hint = Cole a chave aqui
settings-keys-from-environment = Definida por { $variable } no ambiente.
keys-tomtom = TomTom
keys-tomtom-help = Trânsito ao vivo (Camadas → Trânsito).
keys-firms = NASA FIRMS
keys-firms-help = Focos de incêndio vistos por satélite no último dia (Adicionar dados → Biblioteca → Riscos).
settings-category-files = Arquivos e exportação
settings-search-placeholder = Buscar configurações…
settings-search-empty = Nenhuma configuração corresponde a “{ $query }”.
settings-theme-label = Tema
settings-theme-system = Sistema
settings-theme-dark = Escuro
settings-theme-light = Claro
settings-ui-font-label = Fonte da interface
settings-map-font-label = Fonte dos rótulos do mapa
settings-font-system = Padrão do sistema
settings-fonts-loading = Buscando fontes instaladas…
settings-cache-budgets-label = Limites do cache de imagens
settings-cache-decoded-label = Meta de memória decodificada
settings-cache-disk-label = Meta de disco codificado
settings-cache-restart-help = Novos limites valem após reiniciar. A remoção da memória é uma meta aproximada; quadros e outras alocações usam memória adicional. O disco inclui arquivos de imagens gerenciados e sobrecarga SQLite.
settings-cache-borders-label = Cache de dados do mapa no disco (cada conjunto)
settings-cache-borders-usage = Fronteiras e nomes: { $size }; transporte, vias, lugares e pontos de interesse: { $vector_size }. Vale na hora; os blocos usados há mais tempo saem primeiro.
settings-cache-downloads = Baixado nesta sessão: imagens da vista { $imagery } ({ $imagery_tiles } tiles), imagens carregadas antecipadamente { $ahead } ({ $ahead_tiles } tiles), limites e nomes { $borders } ({ $border_tiles } tiles). Lido do cache em disco: { $disk } ({ $disk_tiles } tiles).
settings-cache-resident-label = Cache decodificado atual: { $size }
settings-cache-confirm = Limpar imagens baixadas do mapa? Configurações e documentos geográficos serão mantidos.
settings-confirm-action = Confirmar
settings-cancel-action = Cancelar
settings-reset-confirm = Restaurar os padrões? Documentos geográficos abertos e a escolha de idioma serão mantidos. Limites do cache e sincronização de quadros valem após reiniciar.
settings-pan-sensitivity = Sensibilidade ao arrastar o mapa
settings-rotation-sensitivity = Sensibilidade da rotação ao arrastar
settings-zoom-sensitivity = Sensibilidade do zoom pela roda
settings-invert-scroll = Inverter zoom pela roda
settings-startup-view-label = Vista inicial
settings-startup-current-action = Usar vista atual
settings-startup-default-action = Usar vista padrão
settings-startup-help = Posição, rumo e altitude salvos serão usados na próxima abertura.

settings-network-intro = Hosts que o GlobeCraft pode contatar. Serviços integrados mostram o limite de requisições e o agente de usuário; camadas adicionadas por você e provedores do assistente vêm das suas configurações. Os limites descrevem como o GlobeCraft controla as requisições, não uma garantia do provedor.
settings-network-group-basemaps = Mapas e imagens
settings-network-group-map-data = Dados do mapa
settings-network-group-search = Busca
settings-network-group-live = Camadas ao vivo
settings-network-group-updates = Atualizações
settings-network-group-user = Seus dados
settings-network-group-assistant = Assistente
settings-network-service-wayback = Lista de versões do Esri Wayback
settings-network-service-clouds = Mapas de nuvens ao vivo (Matt Eason, EUMETSAT)
settings-network-service-map-data = Dados de mapa do GlobeCraft
settings-network-service-updates = Atualizações do GlobeCraft
settings-network-limit-parallel = Até { $count } requisições ao mesmo tempo
settings-network-limit-seconds = Normalmente uma requisição a cada { $seconds } s
settings-network-limit-minutes = Normalmente uma requisição a cada { $minutes } min
settings-network-limit-hours = Normalmente uma requisição a cada { $hours } h
settings-network-limit-on-demand = Somente quando necessário
settings-network-limit-once = Uma vez por sessão
settings-network-limit-user-controlled = No intervalo definido por você
settings-network-host-shared = Junto com as outras fontes em { $host }: até { $count } ao mesmo tempo
settings-network-tiles-on-demand = Somente os tiles visíveis
settings-network-tiles-prefetch = Tiles visíveis primeiro; tiles próximos podem ser pré-carregados
settings-network-tile-agent = Requisições de tiles enviam o agente de usuário: { $agent }
settings-network-agent = Agente de usuário: { $agent }
settings-network-agent-updater = A biblioteca de atualização envia o próprio agente de usuário.
settings-network-user-layer-none = Nenhuma camada adicionada por você contata um host de rede.
settings-network-user-layer-host = { $name }: { $host }
settings-network-user-layer-reload = { $name }: { $host } — recarrega a cada { $minutes } min
settings-network-user-layer-help = Estes endereços vêm das camadas que você adicionou. Arquivos neste computador não são enviados.
settings-network-assistant-none = Nenhum provedor de assistente está configurado. O assistente não contata nada até você escolher um.
settings-network-assistant-selected = { $provider }: { $host }
settings-network-assistant-invalid = { $provider }: o endereço configurado não é um host utilizável
settings-network-assistant-help = O assistente usa somente este provedor. Prompts, os comandos propostos e a chave de API vão para ele; um provedor em localhost ou na sua rede local permanece na sua rede.

settings-category-controllers = Controles
controllers-enabled = Ativar controles
controllers-help = Navegação Explorar: analógico esquerdo move, eixo X direito gira e gatilhos controlam o zoom. Solte os controles após fechar diálogos ou reconectar.
controllers-disconnected = Dispositivo selecionado desconectado
controllers-action-none = Sem vínculo
controllers-action-cancel = Cancelar movimento da câmera
controllers-left-x = Analógico esquerdo X
controllers-left-y = Analógico esquerdo Y
controllers-right-x = Analógico direito X
controllers-right-y = Analógico direito Y
controllers-left-trigger = Gatilho esquerdo
controllers-right-trigger = Gatilho direito
controllers-pan-x = Movimento horizontal
controllers-pan-y = Movimento vertical
controllers-heading = Orientação
controllers-zoom-out = Afastar
controllers-zoom-in = Aproximar
controllers-reset = Restaurar perfil do dispositivo selecionado
controllers-listening = Aguardando por 10 segundos… Solte e mova um eixo ou pressione o botão desejado.
controllers-conflicts-help = Eixos podem atender a várias ações intencionalmente. Um vínculo por botão só substitui uma ação existente após confirmação. Os nomes seguem o layout normalizado do dispositivo.
controllers-conflict = { $button } já executa “{ $action }”. Substituir este vínculo?
controllers-replace = Substituir vínculo
controllers-bind = Pressionar para vincular
controllers-invert = Inverter
controllers-deadzone = Zona morta
controllers-sensitivity = Sensibilidade
controllers-curve = Curva de resposta
controllers-buttons = Ações dos botões
controllers-monitor = Entradas normalizadas ao vivo

sky-cinematic = Céu cinematográfico (visível durante o dia)
sky-help = A Via Láctea é o mapa difuso da NASA, preparado como fundo SDR 4K; Estrelas desenha estrelas pontuais sobre ele. Sem catálogo de estrelas selecionável nem navegação astronômica.
sky-loading = Preparando fundo celeste…
sky-retry = Tentar carregar o fundo novamente
sky-short-credit = Céu: NASA SVS · Gaia DR2 (ESA/Gaia/DPAC)
    { $sky }
sky-software-credits = Licenças do software de transformação celeste

# Relevo diretamente no motor Rust (sem navegador)
layers-terrain-toggle = Relevo 3D real (Mapterhorn)
layers-terrain-exaggeration = Exagero vertical
layers-terrain-help = Relevo na superfície do globo. Aproxime para até 150 km de altitude; os dados precisam de internet.

# Interface do GlobeCraft 0.3
ribbon-tab-home = Início
ribbon-tab-layers = Camadas
ribbon-tab-live = Ao vivo
ribbon-tab-view = Exibir
ribbon-tab-places = Lugares
ribbon-tab-settings = Configurações
ribbon-tab-help = Ajuda
layers-globe-title = Relevo e grade
cmd-camera-fly-to = Voar até
cmd-camera-fly-to-help = Voa a câmera até uma latitude e longitude, opcionalmente com altitude e direção.
cmd-camera-set = Ir para XY
cmd-camera-set-help = Move a câmera instantaneamente; parâmetros omitidos mantêm o valor atual.
cmd-search-place = Buscar lugar
cmd-search-place-help = Busca um lugar, endereço ou coordenadas (OpenStreetMap Nominatim) e voa até o melhor resultado.
cmd-camera-zoom-in = Aproximar
cmd-camera-zoom-in-help = Aproxima a câmera da superfície.
cmd-camera-zoom-out = Afastar
cmd-camera-zoom-out-help = Afasta a câmera da superfície.
cmd-camera-reset-north = Norte para cima
cmd-camera-reset-north-help = Gira a visão para o norte ficar para cima (N).
cmd-camera-tilt = Inclinar
cmd-camera-tilt-help = Inclina a vista em direção ao horizonte, ou de volta (Shift+rolagem, Shift+arrastar ou botão direito para cima e para baixo, Shift+Cima/Baixo). Perto do chão a câmera se afasta o quanto a inclinação precisar.
cmd-camera-top-down = Vista de cima
cmd-camera-top-down-help = Volta a olhar diretamente para baixo (U).
cmd-camera-global-view = Visão global
cmd-camera-global-view-help = Afasta até o globo inteiro ficar visível.
cmd-camera-auto-rotate = Rotação automática
cmd-camera-auto-rotate-help = Gira o globo lentamente.
cmd-places-add-center = Marcador
cmd-places-add-center-help = Adiciona um marcador no centro da visão e edita seu nome.
cmd-places-add = Adicionar marcador em
cmd-places-add-help = Adiciona um marcador em uma latitude e longitude.
cmd-places-add-folder = Nova pasta
cmd-places-add-folder-help = Adiciona uma pasta em Meus lugares.
cmd-places-fly-to = Voar até o lugar
cmd-places-fly-to-help = Voa até um marcador pelo nome.
cmd-places-remove = Remover lugar
cmd-places-remove-help = Remove um marcador ou pasta pelo nome.
cmd-places-open = Abrir KML
cmd-places-open-help = Abre um arquivo KML ou KMZ em Meus lugares.
cmd-places-save = Salvar
cmd-places-save-help = Salva Meus lugares no arquivo KML atual.
cmd-places-save-as = Salvar como
cmd-places-save-as-help = Salva Meus lugares em um novo arquivo KML ou KMZ.
cmd-places-clear = Limpar tudo
cmd-places-clear-help = Remove todos os lugares e esquece os arquivos KML abertos (os arquivos são mantidos).
cmd-ruler-enable = Régua
cmd-ruler-enable-help = Mede distâncias geodésicas: clique no globo para adicionar pontos e Alt+clique para iniciar outro caminho.
cmd-ruler-add-point = Adicionar ponto
cmd-ruler-add-point-help = Adiciona um ponto de medição em uma latitude e longitude.
cmd-ruler-clear = Limpar régua
cmd-ruler-clear-help = Remove todos os pontos de medição.
cmd-ruler-close-shape = Fechar forma
cmd-ruler-close-shape-help = Liga cada caminho da régua com três pontos ou mais de volta ao primeiro ponto e mede a área que ele cerca.
cmd-ruler-profile = Perfil de elevação
cmd-ruler-profile-help = A altura do terreno ao longo da régua: ponto mais baixo e mais alto, subida, descida e as alturas pelo caminho.
cmd-ruler-unit = Unidade da régua
cmd-ruler-unit-help = Escolhe a unidade das medições.
cmd-ruler-edit-points = Editar pontos
cmd-ruler-edit-points-help = Digita coordenadas exatas para os pontos da régua.
cmd-edit-undo = Desfazer
cmd-edit-undo-help = Desfaz a última edição da régua.
cmd-edit-redo = Refazer
cmd-edit-redo-help = Refaz a última edição desfeita da régua.
cmd-file-screenshot = Captura
cmd-file-screenshot-help = Salva o globo sem a interface como PNG na pasta de capturas.
cmd-view-copy-link = Copiar link da vista
cmd-view-copy-link-help = Copia um link globecraft:// para esta vista (lugar, altura, ângulo, mapa base, camadas e dados da web). Cole-o na caixa de busca, ou envie a alguém, para abrir a mesma vista.
cmd-view-open-link = Abrir link de vista
cmd-view-open-link-help = Abre um link de vista globecraft://: o lugar, a altura e o ângulo, o mapa base, as camadas e os dados da web.
view-link-copied-status = Link da vista copiado.
view-link-opened-status = Link da vista aberto.
cmd-file-screenshot-quality = Qualidade da captura
cmd-file-screenshot-quality-help = Escolhe a qualidade da captura: Normal (tamanho da tela, quando todos os tiles carregarem), Plus (o dobro do tamanho, um nível de zoom a mais), Pro (quatro vezes o tamanho, dois níveis a mais) ou Pro Max (oito vezes o tamanho, três níveis a mais).
screenshot-quality-normal = Normal
screenshot-quality-plus = Plus
screenshot-quality-pro = Pro
screenshot-quality-pro-max = Pro Max
cmd-file-cancel-screenshot = Cancelar captura
cmd-file-cancel-screenshot-help = Para a captura em andamento; nada é salvo.
screenshot-cancelled-status = Captura cancelada.
screenshot-none-running-error = Nenhuma captura em andamento.
screenshot-rendering-status = Fazendo a captura { $quality }: { $percent } %, { $tiles } tiles a carregar…
cmd-ui-add-data = Adicionar dados
cmd-ui-add-data-help = Adiciona um mapa da web, pontos e linhas de um arquivo ou URL, ou uma fonte da biblioteca.
cmd-data-add = Adicionar camada de dados
cmd-data-add-help = Adiciona como camada uma fonte da biblioteca, um mapa da web ou um feed GeoJSON, GPX ou CSV.
cmd-data-search = Pesquisar biblioteca de dados
cmd-data-search-help = Encontra fontes na biblioteca de Adicionar dados por nome, descrição ou categoria.
cmd-data-show = Mostrar camada do usuário
cmd-data-show-help = Mostra ou oculta uma camada que você adicionou.
cmd-data-opacity = Opacidade da camada do usuário
cmd-data-opacity-help = Define a opacidade de uma camada que você adicionou.
cmd-data-move = Mover camada do usuário
cmd-data-move-help = Desenha uma camada que você adicionou acima ou abaixo da vizinha.
cmd-data-zoom-to = Aproximar da camada do usuário
cmd-data-zoom-to-help = Voa até a área que uma camada de feições cobre.
cmd-data-remove = Remover camada do usuário
cmd-data-remove-help = Remove uma camada que você adicionou.
cmd-info-here = O que há aqui
cmd-info-here-help = Coordenadas em todos os formatos, altitude do terreno, país, estado, município, lugar mais próximo, hora e sol em um ponto.
cmd-ui-here = Mostrar o que há aqui
cmd-ui-here-help = Abre um cartão com tudo o que se sabe sobre um ponto. Clique com o botão direito no globo, ou pressione e segure sobre ele.
cmd-layer-basemap = Mapa base
cmd-layer-basemap-help = Escolhe as imagens aplicadas ao globo.
cmd-layer-wayback-release = Data das imagens
cmd-layer-wayback-release-help = Escolhe uma versão do Esri World Imagery Wayback pela data.
cmd-layer-wayback-compare = Comparar datas
cmd-layer-wayback-compare-help = Desliza entre duas datas de imagens Wayback, uma de cada lado de uma divisória arrastável; escolha qualquer uma das datas ou troque-as.
wayback-compare-label = Comparar com
wayback-compare-tooltip = Deslizar entre duas datas de imagens
wayback-compare-swap-tooltip = Trocar as duas datas
cmd-layer-show = Mostrar camada
cmd-layer-show-help = Mostra, oculta ou alterna uma camada do mapa.
cmd-layer-opacity = Opacidade da camada
cmd-layer-opacity-help = Define a opacidade das camadas de nuvens, radar, aurora, fusos horários ou trânsito (0 a 1).
cmd-layer-solo = Mostrar só esta camada
cmd-layer-solo-help = Mostra só uma camada do conteúdo do mapa (Alt-clique nela no painel Camadas); de novo, ou sem camada, mostra o que estava visível antes.
layers-opacity-label = Opacidade
layers-solo-note = Só { $layer } está visível.
layers-solo-show-all = Mostrar tudo de novo
cmd-layer-terrain-exaggeration = Exagero
cmd-layer-terrain-exaggeration-help = Define o exagero vertical do relevo 3D.
cmd-layer-flat-live-objects = Aeronaves e satélites no chão
cmd-layer-flat-live-objects-help = Desenha aeronaves e satélites planos no chão abaixo deles, em vez de na altura real.
cmd-layer-state-altitude = Estados visíveis abaixo
cmd-layer-state-altitude-help = Define a altitude da câmera abaixo da qual os limites de estados e províncias aparecem.
cmd-world-earth-model = Modelo da Terra
cmd-world-earth-model-help = Escolhe o elipsoide WGS 84 ou uma esfera perfeita para geometria e distâncias.
cmd-world-body = Mundo
cmd-world-body-help = Mostra a Terra, a Lua ou um planeta personalizado.
cmd-ui-explorer = Explorador
cmd-ui-explorer-help = Abre o Explorador: detalhes do lugar, fotos, tours narrados e planetas.
cmd-ui-pane-layers = Painel de camadas
cmd-ui-pane-layers-help = Mostra o painel de camadas.
cmd-ui-pane-places = Painel de lugares
cmd-ui-pane-places-help = Mostra Meus lugares.
cmd-ui-pane-explore = Painel do globo
cmd-ui-pane-explore-help = Mostra resultados de busca, navegação e exportação.
cmd-ui-pane-data = Painel de dados
cmd-ui-pane-data-help = Mostra suas fontes favoritas e recentes, a biblioteca de dados e suas camadas.
cmd-ui-pane-assistant = Assistente de IA
cmd-ui-pane-assistant-help = Faça perguntas sobre o globo ou diga ao GlobeCraft o que fazer, com o modelo que preferir.
cmd-assistant-stop = Parar o assistente de IA
cmd-assistant-stop-help = Encerra a solicitação atual do assistente; as ações ainda não executadas são puladas e uma resposta atrasada é ignorada.
cmd-ui-pane-measure = Painel de medição
cmd-ui-pane-measure-help = Mostra medições, unidades e histórico da régua.
cmd-ui-left-pane = Painel lateral
cmd-ui-left-pane-help = Mostra ou oculta o painel à esquerda do globo.
cmd-ui-navigation-controls = Navegador
cmd-ui-navigation-controls-help = Mostra ou oculta a bússola e o zoom sobre o globo.
ribbon-group-time = Tempo
cmd-ui-timeline = Linha do tempo
cmd-ui-timeline-help = Mostra a linha do tempo: a data e a hora do app, passos, reprodução e a janela de eventos passados.
cmd-time-set = Definir hora
cmd-time-set-help = Mostra o globo numa data e hora, ou de volta à hora real (agora): luz do sol, estrelas, imagens diárias, Wayback e pontos com hora acompanham.
cmd-time-step = Avançar ou recuar
cmd-time-step-help = Move a hora do app algumas horas para a frente ou para trás.
cmd-time-play = Reproduzir tempo
cmd-time-play-help = Faz a hora do app correr numa velocidade, como uma hora por segundo.
cmd-time-pause = Pausar tempo
cmd-time-pause-help = Mantém a hora do app onde está.
cmd-time-speed = Velocidade do tempo
cmd-time-speed-help = Quão rápido o tempo corre quando reproduz, como uma hora por segundo.
cmd-time-window = Janela de eventos
cmd-time-window-help = Mostra pontos com hora (terremotos…) só das horas antes da hora do app.
timeline-live = Ao vivo
timeline-now-action = Agora
timeline-now-help = De volta à hora real
timeline-day-back-help = Um dia para trás
timeline-hour-back-help = Uma hora para trás
timeline-hour-forward-help = Uma hora para a frente
timeline-day-forward-help = Um dia para a frente
timeline-time-of-day-help = Hora do dia (UTC)
timeline-speed-label = Velocidade
timeline-speed-minute = 1 min/s
timeline-speed-hour = 1 h/s
timeline-speed-six-hours = 6 h/s
timeline-speed-day = 1 dia/s
timeline-window-label = Eventos
timeline-window-all = Todos
timeline-window-hour = Última hora
timeline-window-day = Último dia
timeline-window-week = Últimos 7 dias
timeline-window-month = Últimos 30 dias
cmd-ui-tile-diagnostics = Diagnóstico de blocos
cmd-ui-tile-diagnostics-help = Mostra os blocos de imagem na vista por estado, as filas de download por fonte e as taxas de download.
diagnostics-tiles-line = Blocos na vista: { $total } (prontos { $ready }, carregando { $loading }, faltando { $missing }, com falha { $failed })
diagnostics-downloads-line = Downloads em andamento: { $count }
diagnostics-source-line = { $source }: { $queued } na fila, { $downloading } baixando
diagnostics-source-paused-line = { $source }: { $queued } na fila, { $downloading } baixando, pausado pelo servidor
diagnostics-rates-line = Vista { $view } · antecipados { $ahead } · fronteiras { $borders }
cmd-ui-navigator-style = Estilo dos controles de navegação
cmd-ui-navigator-style-help = Escolhe os controles com o miniglobo ou os clássicos: anel, controle de movimento e zoom.
cmd-ui-ribbon = Recolher faixa
cmd-ui-ribbon-help = Recolhe ou expande a faixa de opções (Ctrl+F1).
cmd-ui-theme = Tema
cmd-ui-theme-help = Segue o sistema ou usa o tema escuro ou claro.
cmd-ui-fullscreen = Tela cheia
cmd-ui-fullscreen-help = Entra ou sai da tela cheia.
cmd-ui-minimize = Minimizar
cmd-ui-minimize-help = Minimiza a janela.
cmd-ui-command-search = Busca de comandos
cmd-ui-command-search-help = Encontra e executa qualquer comando pelo nome.
cmd-ui-settings = Configurações
cmd-ui-settings-help = Abre a janela de configurações.
cmd-settings-units = Unidades
cmd-settings-units-help = Mostra distâncias e altitudes em unidades métricas ou imperiais, ou segue o sistema.
cmd-settings-experience = Nível de experiência
cmd-settings-experience-help = Básico mostra as ferramentas do dia a dia; Avançado acrescenta os modelos da Terra, outros mundos e os formulários de serviços web e URLs de feições.
settings-experience-label = Nível de experiência
settings-experience-basic = Básico
settings-experience-advanced = Avançado
add-data-advanced-hint = Serviços web e URLs de feições: mude para Avançado nas Configurações.
cmd-settings-coordinate-format = Coordenadas
cmd-settings-coordinate-format-help = Escolhe como as coordenadas são exibidas.
cmd-settings-language = Idioma
cmd-settings-language-help = Escolhe o idioma da interface.
cmd-ui-shortcuts = Atalhos
cmd-ui-shortcuts-help = Mostra os controles de mouse e teclado.
cmd-help-documentation = Documentação
cmd-help-documentation-help = Abre a documentação do GlobeCraft.
cmd-help-report-issue = Relatar problema
cmd-help-report-issue-help = Relata um problema ou sugere uma ideia.
cmd-ui-about = Sobre
cmd-ui-about-help = Sobre o GlobeCraft e suas fontes de dados.
cmd-app-quit = Sair
cmd-app-quit-help = Fecha o GlobeCraft.
cmd-info-view = Descrever visão
cmd-info-view-help = Posição da câmera, camadas visíveis, mapa base, mundo e unidades.
cmd-info-places = Listar lugares
cmd-info-places-help = Todos os marcadores e pastas de Meus lugares, com coordenadas.
cmd-info-ruler = Medição
cmd-info-ruler-help = Pontos da régua, distância total e último rumo.
assistant-suggestion-fly = Voe até o Monte Everest
assistant-suggestion-layers = Mostre os aviões ao vivo e o radar de chuva
assistant-suggestion-measure = Qual é a distância de Lisboa ao Rio de Janeiro?
assistant-suggestion-placemark = Adicione marcadores para as Sete Maravilhas do Mundo
assistant-suggestion-moon = Leve-me para a Lua
assistant-note-length = A resposta atingiu o limite de tamanho.
assistant-note-filter = O filtro de conteúdo do provedor interrompeu a resposta.
assistant-note-summarising = Resumindo a conversa anterior para caber no contexto do modelo…
assistant-note-compacted = A conversa anterior foi resumida.
assistant-note-empty = O modelo enviou uma resposta vazia; pergunte de novo ou tente outro modelo.
assistant-note-stopped = Interrompido.
assistant-note-stopped-early = A resposta parou cedo ({ $reason }).
assistant-note-declined = O modelo recusou esta solicitação.
assistant-note-declined-reason = O modelo recusou esta solicitação: { $why }
assistant-note-incomplete = A resposta parou antes de terminar; nenhuma ação foi executada.
assistant-note-gemini-stopped = O Gemini interrompeu a resposta ({ $reason }).
assistant-note-http-retry = O provedor retornou HTTP { $status }; tentando novamente em { $seconds } s.
assistant-note-connection-retry = A conexão falhou; tentando novamente em { $seconds } s.
assistant-note-step-limit = Interrompido após { $steps } etapas; diga "continuar" para prosseguir.
assistant-note-call-limit = Interrompido após { $calls } ações para uma só mensagem; diga "continuar" para prosseguir.
assistant-note-repeat-limit = Interrompido: o modelo pediu a mesma ação { $count } vezes seguidas.
assistant-usage = { $input } tokens de entrada, { $output } de saída
assistant-usage-cost = { $input } tokens de entrada, { $output } de saída · cerca de US${ $cost } (estimativa)
assistant-usage-tooltip = Tokens que esta mensagem usou em todas as suas solicitações. O custo é uma estimativa aproximada a partir de preços de tabela por família de modelos; o que vale é a cobrança do seu provedor.
assistant-model-tooltip = O modelo que responde; clique para trocar
assistant-new-chat = Nova conversa
assistant-compact-tooltip = Resume a conversa até aqui para que um chat longo caiba no contexto do modelo
assistant-provider-section = Provedor de IA
assistant-provider-label = Provedor
assistant-model-label = Modelo
assistant-list-models = Listar
assistant-address-label = Endereço
assistant-key-label = Chave de API
assistant-key-optional = opcional
assistant-get-key = Obter uma chave de API
assistant-remember-key = Lembrar chaves de API neste computador
assistant-privacy-note = Suas mensagens, a visão (câmera, camadas, lugares) e os resultados dos comandos são enviados ao provedor escolhido. As chaves só são guardadas se você pedir, legíveis apenas pela sua conta de usuário.
assistant-approve-title = Permitir esta ação?
assistant-approve-reason = O assistente quer executar isto, e { $why }:
assistant-approve-always = Permitir essas ações nesta conversa
assistant-allow = Permitir
assistant-decline = Recusar
assistant-input-hint = Pergunte sobre o globo ou diga o que fazer…
assistant-input-help = Enter para enviar, Shift+Enter para nova linha
assistant-input-help-busy = Esc ou Parar encerra a solicitação
assistant-stop = Parar
assistant-send = Enviar
assistant-intro = Faça uma pergunta sobre os lugares à sua frente ou descreva o que fazer. O assistente executa comandos do GlobeCraft por você: voos, camadas, marcadores, medições e mais.
assistant-thinking = Pensando…
palette-hint = Buscar comandos — ex.: relevo, voar, marcador
palette-empty = Nenhum comando corresponde. Pressione Enter para perguntar ao assistente de IA.
palette-ask-assistant = Perguntar ao assistente de IA: { $question }
palette-layer-show = Mostrar camada
palette-layer-hide = Ocultar camada
ribbon-file = Arquivo
ribbon-file-help = Abrir e salvar lugares, capturas, configurações e sair
ribbon-expand-tooltip = Expandir a faixa de opções (Ctrl+F1)
ribbon-collapse-tooltip = Recolher a faixa de opções (Ctrl+F1)
ribbon-scroll-left = Rolar para a esquerda
ribbon-scroll-right = Rolar para a direita
ribbon-group-navigate = Navegar
ribbon-group-places = Lugares
ribbon-group-measure = Medir
ribbon-group-capture = Capturar
ribbon-group-assistant = IA
ribbon-group-basemap = Mapa base
ribbon-group-pane = Painel
ribbon-group-worlds = Mundos
ribbon-group-panes = Painéis
ribbon-group-window = Janela
ribbon-group-kml = KML / KMZ
ribbon-group-add = Adicionar
ribbon-group-manage = Gerenciar
ribbon-group-preferences = Preferências
ribbon-group-display = Exibição
ribbon-group-commands = Comandos
ribbon-imagery-date = Data das imagens
ribbon-sources-status = Fontes e status
ribbon-basemap-satellite = Satélite
ribbon-basemap-wayback = Wayback
ribbon-basemap-streets = Ruas
ribbon-basemap-topo = Topográfico
ribbon-units-system = Sistema
ribbon-units-metric = Métrico
ribbon-units-imperial = Imperial
ribbon-assistant-provider = Provedor de IA
ribbon-assistant-provider-help = Escolha o modelo usado pelo assistente de IA e informe a chave.
ribbon-layer-countries = Países
ribbon-layer-states = Estados
ribbon-layer-labels = Nomes de lugares
ribbon-layer-municipal = Municípios
ribbon-layer-terrain = Relevo 3D
ribbon-layer-grid = Grade
ribbon-layer-grid-labels = Rótulos da grade
ribbon-layer-atmosphere = Atmosfera
ribbon-layer-sunlight = Luz solar
ribbon-layer-night-lights = Luzes noturnas
ribbon-layer-stars = Estrelas
ribbon-layer-milky-way = Via Láctea
ribbon-layer-cinematic = Céu cinematográfico
ribbon-layer-radar = Radar de chuva
ribbon-layer-clouds = Nuvens
ribbon-layer-flights = Aviões
ribbon-layer-vessels = Navios
ribbon-layer-satellites = Satélites
ribbon-layer-roads = Estradas
ribbon-layer-railways = Ferrovias
ribbon-layer-ferries = Balsas
ribbon-layer-airports = Aeroportos
ribbon-layer-poi = Pontos de interesse
ribbon-layer-peaks = Picos
ribbon-layer-waters = Rios e águas
ribbon-layer-parks = Parques
ribbon-layer-traffic = Trânsito
statusbar-tiles-ready = Blocos prontos
assistant-settings-title = Configurações do assistente
pane-measure-title = Medição
pane-close-tooltip = Fechar o painel
pane-close-named-tooltip = Fechar { $name }
layers-earth-only = As camadas se aplicam à Terra. Volte para a Terra em Exibir › Mundos.
explore-results-empty = Busque um lugar na caixa no topo da janela; os resultados aparecem aqui.
measure-total-label = Distância total
measure-points-label = { $count } pontos
measure-area-label = Área: { $area }
measure-profile-title = Perfil de elevação
measure-profile-loading = Carregando as alturas do terreno…
measure-profile-stats = Mais baixo { $min } · mais alto { $max } · subida { $ascent } · descida { $descent }
measure-profile-hover = { $distance } · { $height }
measure-how-title = Como medir

settings-category-map-style = Estilo do mapa

settings-map-style-line-country = Fronteiras de países

settings-map-style-line-state = Limites de estados e províncias

settings-map-style-line-municipal = Limites municipais
settings-map-style-line-disputed = Fronteiras disputadas
settings-map-style-group-borders = Fronteiras
settings-map-style-group-roads = Estradas
settings-map-style-group-transport = Ferrovias, balsas e aeroportos
settings-map-style-group-water-parks = Rios e parques
settings-map-style-group-time-zones = Fusos horários
settings-map-style-line-major-road = Rodovias e vias principais
settings-map-style-line-minor-road = Vias menores e ruas
settings-map-style-line-roadworks = Obras na via
settings-map-style-line-railway = Ferrovias
settings-map-style-line-service-rail = Pátios e desvios
settings-map-style-line-subway = Metrô
settings-map-style-line-tram = Bondes e VLT
settings-map-style-line-ferry = Balsas
settings-map-style-line-runway = Pistas de pouso
settings-map-style-line-taxiway = Pistas de táxi
settings-map-style-line-river = Rios
settings-map-style-line-canal = Canais
settings-map-style-line-park = Parques e áreas protegidas
settings-map-style-line-time-zone = Linhas dos fusos horários

settings-map-style-line-help = Limites municipais sobre uma divisa estadual usam o estilo dos estados, para que a troca de perto mantenha o mesmo visual.

settings-map-style-color-label = Cor

settings-map-style-width-label = Espessura

settings-map-style-dash-label = Traço

settings-map-style-dash-solid = Contínuo

settings-map-style-dash-dashed = Tracejado

settings-map-style-dash-dotted = Pontilhado

settings-map-style-labels-title = Tamanho dos nomes

settings-map-style-label-country = Países

settings-map-style-label-state = Estados e províncias

settings-map-style-label-city = Cidades e municípios
settings-map-style-label-road = Nomes de ruas
settings-map-style-label-time-zone = Diferenças dos fusos horários

settings-map-style-reset-action = Restaurar estilo do mapa

cmd-style-line = Estilo das linhas do mapa
cmd-style-line-help = Define a cor, a espessura e o traço de um tipo de linha do mapa: fronteiras, estradas, ferrovias e outros transportes, rios, contornos de parques ou linhas dos fusos horários.
cmd-style-label-size = Tamanho dos nomes
cmd-style-label-size-help = Define o tamanho da fonte dos nomes de países, estados, cidades ou ruas, ou das diferenças dos fusos horários.
cmd-style-reset = Restaurar estilo do mapa
cmd-style-reset-help = Restaura as cores, espessuras, traços e tamanhos de nomes padrão.

add-data-title = Adicionar dados
add-data-tab-library = Biblioteca
add-data-tab-web = Mapa da web
add-data-tab-features = Pontos e linhas
add-data-category-ocean = Oceano
add-data-category-earth = Terra
add-data-category-hazards = Riscos
add-data-commercial-yes = uso comercial permitido
add-data-commercial-no = somente uso não comercial
add-data-longevity-institutional = mantido por uma instituição pública
add-data-longevity-goodwill = mantido por boa vontade, sem garantia
add-data-add-action = Adicionar
add-data-added-label = Adicionado
add-data-test-action = Testar
add-data-testing-status = Buscando uma amostra…
add-data-test-failed = Não funcionou: { $error }
add-data-web-xyz = Blocos (XYZ, WMTS)
add-data-web-wms = WMS
add-data-web-archive = Arquivo de blocos (PMTiles, MBTiles)
add-data-web-archive-label = Arquivo
add-data-web-archive-test-ok = O arquivo tem os níveis de zoom { $min } a { $max }; chegou um bloco ({ $width } × { $height } pixels).
add-data-web-archive-help = Um arquivo PMTiles na web (endereço terminando em .pmtiles) ou um arquivo MBTiles neste computador, com imagens PNG, JPEG ou WebP. Testar lê os níveis de zoom; a camada é desenhada sobre as imagens, abaixo das fronteiras.
add-data-web-template-label = Endereço dos blocos
add-data-web-service-label = Endereço do serviço
add-data-web-tile-size-label = Tamanho do bloco
add-data-web-max-zoom-label = Nível mais detalhado
add-data-web-layer-label = Camada
add-data-web-no-layers = Carregue antes as camadas do serviço
add-data-web-load-layers = Carregar camadas
add-data-web-default-name = Mapa da web
add-data-web-test-ok = Chegou um bloco ({ $width } × { $height } pixels).
add-data-web-help = O endereço dos blocos usa {"{"}z{"}"}, {"{"}x{"}"} e {"{"}y{"}"} ({"{"}-y{"}"} para TMS); {"{"}bbox{"}"} e {"{"}date{"}"} (ontem) também são preenchidos. A camada é desenhada sobre as imagens, abaixo das fronteiras.
add-data-size-by-label = Tamanho por
add-data-size-by-hint = Uma propriedade, como mag ou population (encontrada se vazio)
add-data-name-label = Nome
add-data-attribution-label = Crédito
add-data-attribution-hint = O que a licença dos dados pede para mostrar
add-data-location-label = Arquivo ou URL
add-data-location-hint = Um caminho neste computador ou https://…
add-data-browse-action = Procurar…
add-data-file-filter = GeoJSON, GPX ou CSV
add-data-format-label = Formato
add-data-format-auto = Pelo nome do arquivo
add-data-format-geojson = GeoJSON
add-data-format-gpx = GPX
add-data-format-csv = CSV com coordenadas
add-data-format-unknown = Escolha o formato: o nome do arquivo não o indica.
add-data-refresh-label = Recarregar a cada
add-data-refresh-help = Para um feed ao vivo: com que frequência é lido de novo. 0 lê uma vez.
add-data-color-label = Cor
add-data-features-test-ok = Foram lidos { $points } pontos e { $lines } linhas ou contornos.
add-data-kml-help = KML e KMZ abrem em Meus lugares.
add-data-kml-action = Abrir KML…
add-data-added-status = “{ $name }” foi adicionada às suas camadas.
user-point-time = Hora: { $time }
user-cluster-hover = { $count } pontos aqui
layers-user-legend-size = Tamanho por { $field }: { $low } a { $high }
layers-user-legend = { $points ->
        [one] 1 ponto
       *[other] { $points } pontos
    } · { $lines ->
        [one] 1 linha ou contorno
       *[other] { $lines } linhas ou contornos
    }
layers-user-title = Meus dados
layers-user-empty = Nada adicionado ainda.
layers-user-add-action = Adicionar dados…
layers-user-loading = Carregando…
layers-user-failed = Não está carregando: { $error }
layers-user-opacity-label = Opacidade
layers-user-up-help = Desenhar sobre a camada acima
layers-user-down-help = Desenhar sob a camada abaixo
layers-user-zoom-help = Voar até a área que ela cobre
layers-user-remove-help = Remover esta camada
preset-gebco = Fundo do mar (GEBCO)
preset-gebco-help = Relevo sombreado do fundo do oceano e da terra, no mundo todo.
preset-territorial = Mar territorial (12 mn)
preset-territorial-help = O limite exterior do mar territorial de cada país, a 12 milhas náuticas das linhas de base: suas águas soberanas.
preset-contiguous = Zonas contíguas (24 mn)
preset-contiguous-help = O limite exterior da zona contígua, a 24 milhas náuticas, onde um país pode aplicar leis aduaneiras, de imigração e sanitárias.
preset-shelves = Plataformas continentais estendidas
preset-shelves-help = Fundo marinho reivindicado além de 200 milhas náuticas, conforme submetido e recomendado pela Comissão de Limites da Plataforma Continental da ONU.
preset-lme = Grandes ecossistemas marinhos
preset-lme-help = Os 66 grandes ecossistemas marinhos: regiões costeiras distinguidas pelo fundo, correntes e cadeias alimentares.
preset-fao = Áreas de pesca da FAO
preset-fao-help = As principais áreas de pesca que a Organização das Nações Unidas para a Alimentação e a Agricultura usa para registrar capturas.
preset-heritage-marine = Sítios marinhos do Patrimônio Mundial
preset-heritage-marine-help = Sítios do Patrimônio Mundial da UNESCO no mar: recifes, ilhas e costas como a Grande Barreira de Coral e Galápagos.
preset-gdacs = Alertas de desastres (GDACS)
preset-gdacs-help = Alertas atuais e recentes de ciclones tropicais, terremotos, enchentes, vulcões, secas e incêndios florestais, dimensionados pelo nível de alerta. Atualizado a cada 30 minutos.
preset-power-plants = Usinas de energia (WRI)
preset-power-plants-help = Cerca de 35.000 usinas no mundo, de carvão a eólica, dimensionadas pela capacidade. Dados de 2021.
preset-fires = Focos de incêndio (NASA FIRMS)
preset-fires-help = Incêndios e outros pontos quentes vistos pelo satélite VIIRS no último dia, dimensionados pela potência radiativa. Atualizado a cada 30 minutos.
add-data-key-needed = Precisa de uma chave gratuita { $service }: adicione em Configurações → Chaves.
add-data-key-ready = Usa sua chave { $service } de Configurações → Chaves.
preset-eez = Fronteiras marítimas (ZEE)
preset-eez-help = Zonas econômicas exclusivas: onde terminam as águas de cada país, a 200 milhas náuticas ou a meio caminho do vizinho; além delas fica o alto-mar. Indicativas, não para navegação.
preset-iho = Mares e oceanos (OHI)
preset-iho-help = Os limites dos oceanos e mares conforme a Organização Hidrográfica Internacional.
preset-emodnet = Profundidade dos mares europeus (EMODnet)
preset-emodnet-help = Profundidade da água dos mares europeus em cores, mais detalhada que o GEBCO ali.
preset-gibs = Ontem visto do espaço (NASA GIBS)
preset-gibs-help = A Terra inteira como o satélite MODIS Terra a viu ontem, com as nuvens.
preset-forest-loss = Perda de cobertura florestal (Global Forest Watch)
preset-forest-loss-help = Floresta perdida desde 2001, a partir do Landsat (Hansen, Universidade de Maryland).
preset-earthquakes = Terremotos da última semana (USGS)
preset-earthquakes-help = Terremotos de magnitude 2,5 ou mais nos últimos sete dias, lidos de novo a cada cinco minutos.
globe-whats-here-action = O que há aqui?
here-title = O que há aqui
here-copy-help = Copiar
here-elevation-label = Altitude do terreno
here-loading = Carregando…
here-country-label = País
here-no-country = Nenhum (mar ou terra não reivindicada)
here-state-label = Estado
here-municipality-label = Município
here-municipality-help = Recebe o nome da cidade mais importante dentro dos mesmos limites municipais, em geral a sede do município.
here-state-not-loaded-help = Os limites estaduais em volta deste ponto não estão carregados. Eles carregam em volta da vista: olhe o ponto mais de perto.
here-municipality-not-loaded-help = Os limites municipais só carregam com a vista aproximada do ponto.
here-admin-no-name-help = Nenhum nome fica dentro dos mesmos limites que o ponto nos limites carregados até agora.
here-nearest-label = Lugar mais próximo
here-nearest-value = { $distance } a { $direction } de { $name }
here-time-label = Hora
here-time-value = { $solar } solar · { $utc } UTC · fuso náutico { $zone }
here-time-help = A hora solar média segue a longitude; o fuso horário legal do lugar pode diferir em uma hora ou mais.
here-sun-label = Sol
here-sun-up = { $degrees }° acima do horizonte
here-sun-down = { $degrees }° abaixo do horizonte
