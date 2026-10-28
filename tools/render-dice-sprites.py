"""Reproduce transparent Sprøyt dice sprites with Blender 5 (no AI pip counts)."""
import bpy
import math
from pathlib import Path
from mathutils import Vector, Quaternion

out = Path(__file__).resolve().parents[1] / 'assets' / 'dice'
out.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.render.engine = 'CYCLES'
scene.cycles.samples = 24
scene.cycles.use_denoising = True
scene.render.resolution_x = scene.render.resolution_y = 256
scene.render.resolution_percentage = 100
scene.render.film_transparent = True
scene.render.image_settings.file_format = 'PNG'
scene.render.image_settings.color_mode = 'RGBA'
scene.world = bpy.data.worlds.new('Soft studio')
scene.world.use_nodes = True
scene.world.node_tree.nodes['Background'].inputs['Color'].default_value = (0.8, 0.8, 0.8, 1)
scene.world.node_tree.nodes['Background'].inputs['Strength'].default_value = 0.4

def material(name, color, roughness):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = color
    mat.use_nodes = True
    shader = mat.node_tree.nodes.get('Principled BSDF')
    shader.inputs['Base Color'].default_value = color
    shader.inputs['Roughness'].default_value = roughness
    return mat

ivory = material('Warm porcelain', (0.91, 0.90, 0.84, 1), 0.30)
ink = material('Inset charcoal pips', (0.012, 0.016, 0.012, 1), 0.42)
root = bpy.data.objects.new('Rolled die', None)
scene.collection.objects.link(root)
bpy.ops.mesh.primitive_cube_add(size=2)
body = bpy.context.object
body.name = 'Rounded porcelain die'
body.parent = root
body.data.materials.append(ivory)
bevel = body.modifiers.new('Soft edges', 'BEVEL')
bevel.width = 0.17
bevel.segments = 5
body.modifiers.new('Weighted normals', 'WEIGHTED_NORMAL')
for poly in body.data.polygons: poly.use_smooth = True

patterns = {
    1: [(0,0)],
    2: [(-1,-1),(1,1)],
    3: [(-1,-1),(0,0),(1,1)],
    4: [(-1,-1),(-1,1),(1,-1),(1,1)],
    5: [(-1,-1),(-1,1),(0,0),(1,-1),(1,1)],
    6: [(-1,-1),(-1,0),(-1,1),(1,-1),(1,0),(1,1)],
}
# Opposite faces always sum to seven. The labelled face is rotated upward.
faces = {1:(0,0,1),6:(0,0,-1),2:(0,1,0),5:(0,-1,0),3:(1,0,0),4:(-1,0,0)}
for value, normal in faces.items():
    n = Vector(normal)
    u = Vector((1,0,0)) if abs(n.z)>0.5 else Vector((0,0,1))
    v = n.cross(u)
    for x,y in patterns[value]:
        pos=n*0.999 + u*(x*0.43) + v*(y*0.43)
        bpy.ops.mesh.primitive_uv_sphere_add(segments=16,ring_count=8,radius=0.145,location=pos)
        pip=bpy.context.object
        pip.name=f'Face {value} pip'
        pip.scale.z=0.12
        pip.rotation_mode='QUATERNION'
        pip.rotation_quaternion=Vector((0,0,1)).rotation_difference(n)
        pip.parent=root
        pip.data.materials.append(ink)
        for poly in pip.data.polygons: poly.use_smooth=True

bpy.ops.object.camera_add(location=(5,-7,9))
camera=bpy.context.object
camera.rotation_euler=(Vector((0,0,0))-camera.location).to_track_quat('-Z','Y').to_euler()
camera.data.type='ORTHO'
camera.data.ortho_scale=3.75
scene.camera=camera
for name,loc,power,size in [('Key',(-3,-4,7),550,5),('Fill',(4,1,4),250,4)]:
    bpy.ops.object.light_add(type='AREA',location=loc)
    light=bpy.context.object
    light.name=name
    light.data.energy=power
    light.data.shape='DISK'
    light.data.size=size
    light.rotation_euler=(-light.location).to_track_quat('-Z','Y').to_euler()

root.rotation_mode='QUATERNION'
for value,normal in faces.items():
    up=Vector(normal).rotation_difference(Vector((0,0,1)))
    for variant,yaw in [('a',-0.22),('b',0.28)]:
        root.rotation_quaternion=Quaternion(Vector((0,0,1)),yaw) @ up
        scene.render.filepath=str(out / f'{value}-{variant}.png')
        bpy.ops.render.render(write_still=True)
print('Dice sprites rendered:',out)
