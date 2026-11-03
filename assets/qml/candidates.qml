<!DOCTYPE qgis PUBLIC 'http://mrcc.com/qgis.dtd' 'SYSTEM'>
<qgis version="3.34.0-Prizren" styleCategories="Symbology|Labeling">
 <renderer-v2 type="graduatedSymbol" attr="score_norm" graduatedMethod="GraduatedColor" symbollevels="0" enableorderby="1" forceraster="0" referencescale="-1">
  <ranges>
   <range lower="0.000000" upper="0.500000" symbol="0" label="0 – 0.5" render="true"/>
   <range lower="0.500000" upper="0.800000" symbol="1" label="0.5 – 0.8" render="true"/>
   <range lower="0.800000" upper="0.950000" symbol="2" label="0.8 – 0.95" render="true"/>
   <range lower="0.950000" upper="1.000000" symbol="3" label="0.95 – 1 (best)" render="true"/>
  </ranges>
  <symbols>
   <symbol type="marker" name="0" alpha="1" clip_to_extent="1" force_rhr="0"><layer class="SimpleMarker" enabled="1" locked="0" pass="0"><Option type="Map"><Option name="name" type="QString" value="circle"/><Option name="size" type="QString" value="2.6"/><Option name="color" type="QString" value="70,98,215,255"/><Option name="outline_color" type="QString" value="255,255,255,255"/><Option name="outline_width" type="QString" value="0.4"/></Option></layer></symbol>
   <symbol type="marker" name="1" alpha="1" clip_to_extent="1" force_rhr="0"><layer class="SimpleMarker" enabled="1" locked="0" pass="0"><Option type="Map"><Option name="name" type="QString" value="circle"/><Option name="size" type="QString" value="3.2"/><Option name="color" type="QString" value="162,252,60,255"/><Option name="outline_color" type="QString" value="255,255,255,255"/><Option name="outline_width" type="QString" value="0.4"/></Option></layer></symbol>
   <symbol type="marker" name="2" alpha="1" clip_to_extent="1" force_rhr="0"><layer class="SimpleMarker" enabled="1" locked="0" pass="0"><Option type="Map"><Option name="name" type="QString" value="circle"/><Option name="size" type="QString" value="3.8"/><Option name="color" type="QString" value="250,187,57,255"/><Option name="outline_color" type="QString" value="255,255,255,255"/><Option name="outline_width" type="QString" value="0.4"/></Option></layer></symbol>
   <symbol type="marker" name="3" alpha="1" clip_to_extent="1" force_rhr="0"><layer class="SimpleMarker" enabled="1" locked="0" pass="0"><Option type="Map"><Option name="name" type="QString" value="star"/><Option name="size" type="QString" value="5.5"/><Option name="color" type="QString" value="228,70,10,255"/><Option name="outline_color" type="QString" value="255,255,255,255"/><Option name="outline_width" type="QString" value="0.5"/></Option></layer></symbol>
  </symbols>
  <orderby><orderByClause asc="1" nullsFirst="0">"score_norm"</orderByClause></orderby>
 </renderer-v2>
 <labeling type="simple">
  <settings calloutType="simple">
   <text-style fieldName="'#' || &quot;rank&quot;" isExpression="1" fontSize="9" textColor="20,20,20,255"><text-buffer bufferDraw="1" bufferSize="0.8" bufferColor="255,255,255,255"/></text-style>
   <placement placement="1" dist="1.5"/>
   <rendering obstacle="1" scaleVisibility="0"/>
  </settings>
 </labeling>
 <blendMode>0</blendMode>
 <featureBlendMode>0</featureBlendMode>
 <layerOpacity>1</layerOpacity>
</qgis>
