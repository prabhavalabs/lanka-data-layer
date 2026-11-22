"""Extract Overture places in the Sri Lanka bbox to GeoJSON (one FeatureCollection).

The overture-maps connector (registry/connectors/overture-maps.yaml, geo_file driver) reads the file this writes:

    pip install duckdb==2.0.1
    python3 overture_places.py 2026-11-18.0 DIR/overture-places-lka.geojson
    lanka ingest run overture-maps --from DIR

Every place whose bbox starts inside 79.4-82.0 E, 5.8-10.0 N is written with its id, primary name, taxonomy,
confidence (+ a band the rules filter on), operating status, contributing datasets and licences; the ingest drops
places outside the country. No value is changed.
"""
import sys, duckdb
rel, out = sys.argv[1], sys.argv[2]
c = duckdb.connect()
c.sql("INSTALL spatial; LOAD spatial; INSTALL httpfs; LOAD httpfs; SET s3_region='us-west-2'; SET geometry_always_xy = true")
c.sql(f"""
COPY (
  SELECT id,
         names."primary" AS name,
         basic_category,
         taxonomy."primary" AS taxonomy_primary,
         taxonomy.hierarchy[1] AS taxonomy_top,
         taxonomy.hierarchy[2] AS taxonomy_l2,
         array_to_string(taxonomy.hierarchy, '>') AS taxonomy_path,
         round(confidence, 4) AS confidence,
         CASE WHEN confidence >= 0.7 THEN 'high' WHEN confidence >= 0.5 THEN 'medium' ELSE 'low' END AS confidence_band,
         operating_status,
         array_to_string(list_distinct(list_transform(sources, lambda s: s.dataset)), ';') AS source_datasets,
         array_to_string(list_distinct(list_transform(sources, lambda s: s.license)), ';') AS source_licences,
         addresses[1].country AS country,
         '{rel}' AS release,
         geometry
  FROM read_parquet('s3://overturemaps-us-west-2/release/{rel}/theme=places/type=place/*', hive_partitioning=1)
  WHERE bbox.xmin BETWEEN 79.4 AND 82.0 AND bbox.ymin BETWEEN 5.8 AND 10.0
) TO '{out}' WITH (FORMAT GDAL, DRIVER 'GeoJSON', LAYER_CREATION_OPTIONS 'COORDINATE_PRECISION=6')
""")
print(c.sql(f"SELECT count(*) FROM st_read('{out}')").fetchall())
